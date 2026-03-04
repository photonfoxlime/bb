//! LLM HTTP client and request/response types.
//!
//! Supports two wire formats via [`ApiStyle`]:
//! - **OpenAI** chat completions API (`/chat/completions`)
//! - **Anthropic** Messages API (`/messages`)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use crate::llm::config::{ApiStyle, LlmConfig};
use crate::llm::context::{
    AmplifyResult, AmplifySuggestion, AtomizeResult, BlockContext, DistillResult,
};
use crate::llm::error::{ApiError, LlmError};
use crate::llm::prompt::{Prompt, TaskPromptConfig};
use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::futures::StreamExt;

/// Incremental probe stream event emitted by [`LlmClient::probe_stream`].
#[derive(Debug)]
pub enum ProbeStreamEvent {
    /// One response delta chunk from the model.
    Chunk(String),
    /// Non-recoverable error while producing inquiry chunks.
    Failed(LlmError),
    /// Terminal event emitted exactly once when the request ends.
    Finished,
}

/// HTTP client for LLM endpoints.
///
/// Dispatches to OpenAI or Anthropic wire formats based on [`ApiStyle`]
/// stored in the [`LlmConfig`].
///
/// # Invariants
/// - The client is stateless aside from the config.
/// - Safe to construct per-request or share across requests.
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    /// Create a new LLM client with the given configuration.
    pub fn new(config: LlmConfig) -> Self {
        Self { config, http: reqwest::Client::new() }
    }

    /// Distill a block's point using ancestor lineage and existing children as context.
    ///
    /// When existing children are present, the LLM may identify some as
    /// redundant (their content is subsumed by the reduction).
    ///
    /// `max_tokens` caps the completion length (`None` = unlimited, omits the
    /// field from the API request).
    ///
    /// # Requires
    /// - `context` must not be empty (must have a lineage).
    ///
    /// # Ensures
    /// - Returns `Ok(DistillResult)` with the condensed text and indices of redundant children.
    /// - If response parsing fails, falls back to plain-text reduction with no redundant children.
    pub async fn distill_block(
        &self, context: &BlockContext, instruction: Option<&str>, max_tokens: Option<u32>,
        config: &TaskPromptConfig,
    ) -> Result<DistillResult, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let has_children =
            !context.existing_children.is_empty() || !context.friend_blocks.is_empty();
        let prompt = Prompt::from_context(config, context, instruction);
        let content = self.request_completion("distill", &prompt, 0.2, max_tokens).await?;

        if has_children {
            // Try structured JSON first; fall back to plain-text distillation.
            if let Ok(payload) = serde_json::from_str::<DistillResponsePayload>(&content) {
                let reduction = payload.reduction.trim().to_string();
                if reduction.is_empty() {
                    return Err(LlmError::InvalidDistillResponse);
                }
                let child_count = context.existing_children.len();
                let redundant_children: Vec<usize> = payload
                    .redundant_children
                    .into_iter()
                    .filter(|&i| i < child_count)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                tracing::info!(
                    chars = reduction.len(),
                    redundant = redundant_children.len(),
                    "llm reduce response (structured)"
                );
                return Ok(DistillResult::new(reduction, redundant_children));
            }
        }

        // Plain-text fallback (no children or JSON parse failed).
        let reduction = content.trim().to_string();
        if reduction.is_empty() {
            return Err(LlmError::InvalidDistillResponse);
        }
        tracing::info!(chars = reduction.len(), "llm distill response (plain)");
        Ok(DistillResult::new(reduction, vec![]))
    }

    /// Amplify one target point into rewrite and concise child point candidates.
    ///
    /// When existing children are present, the prompt instructs the LLM to
    /// avoid suggesting children that overlap with them.
    ///
    /// `max_tokens` caps the completion length (`None` = unlimited, omits the
    /// field from the API request).
    ///
    /// # Requires
    /// - `context` must not be empty (must have a lineage).
    ///
    /// # Ensures
    /// - Returns `Ok(AmplifyResult)` with optional rewrite and child suggestions.
    /// - Returns `Err(LlmError::InvalidAmplifyResponse)` if the response cannot be parsed.
    pub async fn amplify_block(
        &self, context: &BlockContext, instruction: Option<&str>, max_tokens: Option<u32>,
        config: &TaskPromptConfig,
    ) -> Result<AmplifyResult, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::from_context(config, context, instruction);
        let content = self.request_completion("amplify", &prompt, 0.7, max_tokens).await?;
        let payload: AmplifyResponsePayload =
            serde_json::from_str(&content).map_err(|_| LlmError::InvalidAmplifyResponse)?;

        let rewrite =
            payload.rewrite.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
        let children = payload
            .children
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(AmplifySuggestion::new)
            .collect::<Vec<_>>();

        if rewrite.is_none() && children.is_empty() {
            return Err(LlmError::InvalidAmplifyResponse);
        }

        tracing::info!(
            rewrite = rewrite.is_some(),
            children = children.len(),
            "llm amplify response"
        );
        Ok(AmplifyResult::new(rewrite, children))
    }

    /// Atomize one target point into distinct information points.
    ///
    /// Breaks the text into a list of self-contained facts/ideas without
    /// dropping details.
    ///
    /// `max_tokens` caps the completion length (`None` = unlimited, omits the
    /// field from the API request).
    ///
    /// # Requires
    /// - `context` must not be empty (must have a lineage).
    ///
    /// # Ensures
    /// - Returns `Ok(AtomizeResult)` with the list of points.
    /// - Returns `Err(LlmError::InvalidAtomizeResponse)` if the response cannot be parsed.
    pub async fn atomize_block(
        &self, context: &BlockContext, instruction: Option<&str>, max_tokens: Option<u32>,
        config: &TaskPromptConfig,
    ) -> Result<AtomizeResult, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::from_context(config, context, instruction);
        let content = self.request_completion("atomize", &prompt, 0.2, max_tokens).await?;
        let payload: AtomizeResponsePayload =
            serde_json::from_str(&content).map_err(|_| LlmError::InvalidAtomizeResponse)?;

        let rewrite = payload.rewrite.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());

        let points = payload
            .points
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();

        if rewrite.is_none() && points.is_empty() {
            return Err(LlmError::InvalidAtomizeResponse);
        }

        tracing::info!(rewrite = rewrite.is_some(), points = points.len(), "llm atomize response");
        Ok(AtomizeResult::new(rewrite, points))
    }

    /// Probe: send a targeted question to clarify meaning, fill gaps, or challenge
    /// assumptions. One-shot (non-streaming) variant.
    ///
    /// The instruction is sent as a user message with the block context.
    /// Returns a free-form response that can be applied as rewrite, append, or child.
    ///
    /// `max_tokens` caps the completion length (`None` = unlimited, omits the
    /// field from the API request).
    ///
    /// # Requires
    /// - `context` must not be empty (must have a lineage).
    /// - `instruction` must not be empty.
    ///
    /// # Ensures
    /// - Returns `Ok(String)` with a non-empty trimmed LLM response.
    /// - Returns `Err(LlmError::InvalidResponse)` if no usable text is returned.
    pub async fn inquire(
        &self, context: &BlockContext, instruction: &str, max_tokens: Option<u32>,
        config: &TaskPromptConfig,
    ) -> Result<String, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }
        if instruction.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::from_context(config, context, Some(instruction));
        let content = self.request_completion("inquire", &prompt, 0.7, max_tokens).await?;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(LlmError::InvalidResponse);
        }

        tracing::info!(chars = trimmed.len(), "llm inquire response");
        Ok(trimmed.to_string())
    }

    /// Stream probe response chunks as they are produced by the model.
    ///
    /// Probe: ask targeted questions to clarify meaning, fill gaps, or challenge
    /// assumptions. Prefers true server-sent-event streaming (`stream: true`). If
    /// the provider does not support streaming and no chunks were emitted, falls
    /// back to a one-shot probe request and emits that full response as one chunk.
    ///
    /// `max_tokens` caps the completion length (`None` = unlimited, omits the
    /// field from the API request).
    ///
    /// # Requires
    /// - `context` must not be empty (must have a lineage).
    /// - `instruction` must not be empty (the question to probe with).
    ///
    /// # Ensures
    /// - Emits zero or more [`ProbeStreamEvent::Chunk`] events.
    /// - Emits exactly one terminal [`ProbeStreamEvent::Finished`] event.
    /// - Emits [`ProbeStreamEvent::Failed`] before `Finished` on failure.
    pub fn probe_stream(
        self, context: BlockContext, instruction: String, timeout: Duration,
        max_tokens: Option<u32>, config: TaskPromptConfig,
    ) -> impl Stream<Item = ProbeStreamEvent> {
        iced::stream::channel(64, async move |mut output| {
            let request = async {
                if context.is_empty() || instruction.is_empty() {
                    return Err(LlmError::InvalidRequest);
                }

                let prompt = Prompt::from_context(&config, &context, Some(&instruction));

                match self.stream_inquiry_chunks(&prompt, &mut output, max_tokens).await {
                    | Ok(stats) if stats.has_output() => {
                        tracing::info!(
                            chunks = stats.chunk_count,
                            chars = stats.char_count,
                            "llm inquire streaming completed"
                        );
                        Ok(())
                    }
                    | Ok(_) => {
                        tracing::warn!(
                            "llm inquire streaming emitted no chunks; retrying with one-shot request"
                        );
                        self.emit_inquiry_fallback(
                            &context,
                            &instruction,
                            &mut output,
                            max_tokens,
                            &config,
                        )
                        .await
                    }
                    | Err(err) if should_fallback_to_non_stream(&err) => {
                        tracing::warn!(
                            error = %err,
                            "llm inquire streaming unsupported; retrying with one-shot request"
                        );
                        self.emit_inquiry_fallback(
                            &context,
                            &instruction,
                            &mut output,
                            max_tokens,
                            &config,
                        )
                        .await
                    }
                    | Err(err) => Err(err),
                }
            };

            let result = match tokio::time::timeout(timeout, request).await {
                | Ok(result) => result,
                | Err(_) => Err(LlmError::Timeout),
            };

            if let Err(err) = result {
                let _ = output.send(ProbeStreamEvent::Failed(err)).await;
            }

            let _ = output.send(ProbeStreamEvent::Finished).await;
        })
    }

    async fn request_completion(
        &self, purpose: &'static str, prompt: &Prompt, temperature: f32,
        max_completion_tokens: Option<u32>,
    ) -> Result<String, LlmError> {
        let url = self.endpoint_url();
        tracing::info!(model = %self.config.model, url = %url, purpose, ?max_completion_tokens, "llm request");
        let (value, body) =
            self.send_completion_request(&url, prompt, temperature, max_completion_tokens).await?;

        if let Some(content) = extract_completion_content_from_chat_value(&value) {
            tracing::info!(purpose, chars = content.len(), "llm completion response");
            return Ok(content);
        }

        // Retry with a higher limit only when the response was truncated and
        // we have a finite cap that can be raised.
        if response_hit_token_limit(&value) {
            if let Some(current) = max_completion_tokens {
                let retry_max_tokens = (current.saturating_mul(2)).min(2_000);
                if retry_max_tokens > current {
                    tracing::warn!(
                        purpose,
                        first_max_completion_tokens = current,
                        retry_max_completion_tokens = retry_max_tokens,
                        "llm response reached token limit with no extractable text; retrying once"
                    );
                    let (retry_value, retry_body) = self
                        .send_completion_request(&url, prompt, temperature, Some(retry_max_tokens))
                        .await?;
                    if let Some(content) = extract_completion_content_from_chat_value(&retry_value)
                    {
                        tracing::info!(
                            purpose,
                            chars = content.len(),
                            max_completion_tokens = retry_max_tokens,
                            "llm completion response after token-limit retry"
                        );
                        return Ok(content);
                    }
                    tracing::error!(
                        purpose,
                        body_preview = %preview_body(&retry_body),
                        finish_reason = ?first_choice_finish_reason(&retry_value),
                        completion_tokens = ?completion_tokens(&retry_value),
                        "llm retry response still has no extractable text"
                    );
                    return Err(LlmError::InvalidResponse);
                }
            }
            // When unlimited (None), there is no higher limit to try.
        }

        tracing::error!(
            purpose,
            body_preview = %preview_body(&body),
            finish_reason = ?first_choice_finish_reason(&value),
            completion_tokens = ?completion_tokens(&value),
            "llm response json parsed but no text content could be extracted"
        );
        Err(LlmError::InvalidResponse)
    }

    async fn emit_inquiry_fallback(
        &self, context: &BlockContext, instruction: &str,
        output: &mut iced::futures::channel::mpsc::Sender<ProbeStreamEvent>,
        max_tokens: Option<u32>, config: &TaskPromptConfig,
    ) -> Result<(), LlmError> {
        let content = self.inquire(context, instruction, max_tokens, config).await?;
        let _ = output.send(ProbeStreamEvent::Chunk(content.clone())).await;
        tracing::info!(chars = content.len(), "llm inquire fallback response");
        Ok(())
    }

    async fn stream_inquiry_chunks(
        &self, prompt: &Prompt,
        output: &mut iced::futures::channel::mpsc::Sender<ProbeStreamEvent>,
        max_tokens: Option<u32>,
    ) -> Result<StreamStats, LlmError> {
        let url = self.endpoint_url();
        tracing::info!(model = %self.config.model, url = %url, purpose = "inquire", "llm streaming request");
        let response =
            self.send_streaming_completion_request(&url, prompt, 0.7, max_tokens).await?;
        let mut bytes = response.bytes_stream();
        let mut decoder = SseDataDecoder::new();
        let mut stats = StreamStats::default();
        let mut done = false;

        while let Some(chunk_result) = bytes.next().await {
            let chunk = chunk_result?;
            for event_payload in decoder.push_bytes(&chunk) {
                match parse_stream_event_payload(&event_payload)? {
                    | ParsedStreamEvent::Done => {
                        done = true;
                        break;
                    }
                    | ParsedStreamEvent::Delta(delta) => {
                        stats.record_chunk(&delta);
                        if output.send(ProbeStreamEvent::Chunk(delta)).await.is_err() {
                            return Ok(stats);
                        }
                    }
                    | ParsedStreamEvent::Ignore => {}
                }
            }
            if done {
                break;
            }
        }

        if !done {
            for event_payload in decoder.finish() {
                match parse_stream_event_payload(&event_payload)? {
                    | ParsedStreamEvent::Done => break,
                    | ParsedStreamEvent::Delta(delta) => {
                        stats.record_chunk(&delta);
                        if output.send(ProbeStreamEvent::Chunk(delta)).await.is_err() {
                            return Ok(stats);
                        }
                    }
                    | ParsedStreamEvent::Ignore => {}
                }
            }
        }

        Ok(stats)
    }

    async fn send_completion_request(
        &self, url: &str, prompt: &Prompt, temperature: f32, max_completion_tokens: Option<u32>,
    ) -> Result<(Value, String), LlmError> {
        let response = match self.config.api_style {
            | ApiStyle::OpenAi => {
                let request = OpenAiChatRequest {
                    model: self.config.model.clone(),
                    messages: vec![
                        OpenAiMessage { role: OpenAiRole::System, content: prompt.system.clone() },
                        OpenAiMessage { role: OpenAiRole::User, content: prompt.user.clone() },
                    ],
                    temperature,
                    max_completion_tokens,
                    stream: None,
                };
                self.http.post(url).bearer_auth(&self.config.api_key).json(&request).send().await?
            }
            | ApiStyle::Anthropic => {
                let request = AnthropicMessagesRequest {
                    model: self.config.model.clone(),
                    system: prompt.system.clone(),
                    messages: vec![AnthropicMessage {
                        role: AnthropicRole::User,
                        content: prompt.user.clone(),
                    }],
                    max_tokens: max_completion_tokens.unwrap_or(4096),
                    temperature: Some(temperature),
                    stream: None,
                };
                self.http
                    .post(url)
                    .header("x-api-key", &self.config.api_key)
                    .header("anthropic-version", ANTHROPIC_API_VERSION)
                    .header("content-type", "application/json")
                    .json(&request)
                    .send()
                    .await?
            }
        };

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(ApiError { status, body }.into());
        }
        let value: Value = match serde_json::from_str(&body) {
            | Ok(value) => value,
            | Err(err) => {
                tracing::error!(
                    error = %err,
                    body_preview = %preview_body(&body),
                    "llm response is not valid json"
                );
                return Err(LlmError::InvalidResponse);
            }
        };
        Ok((value, body))
    }

    async fn send_streaming_completion_request(
        &self, url: &str, prompt: &Prompt, temperature: f32, max_completion_tokens: Option<u32>,
    ) -> Result<reqwest::Response, LlmError> {
        let response = match self.config.api_style {
            | ApiStyle::OpenAi => {
                let request = OpenAiChatRequest {
                    model: self.config.model.clone(),
                    messages: vec![
                        OpenAiMessage { role: OpenAiRole::System, content: prompt.system.clone() },
                        OpenAiMessage { role: OpenAiRole::User, content: prompt.user.clone() },
                    ],
                    temperature,
                    max_completion_tokens,
                    stream: Some(true),
                };
                self.http.post(url).bearer_auth(&self.config.api_key).json(&request).send().await?
            }
            | ApiStyle::Anthropic => {
                let request = AnthropicMessagesRequest {
                    model: self.config.model.clone(),
                    system: prompt.system.clone(),
                    messages: vec![AnthropicMessage {
                        role: AnthropicRole::User,
                        content: prompt.user.clone(),
                    }],
                    max_tokens: max_completion_tokens.unwrap_or(4096),
                    temperature: Some(temperature),
                    stream: Some(true),
                };
                self.http
                    .post(url)
                    .header("x-api-key", &self.config.api_key)
                    .header("anthropic-version", ANTHROPIC_API_VERSION)
                    .header("content-type", "application/json")
                    .json(&request)
                    .send()
                    .await?
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError { status, body }.into());
        }
        Ok(response)
    }

    /// Endpoint URL for the configured API style.
    fn endpoint_url(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        match self.config.api_style {
            | ApiStyle::OpenAi => format!("{base}/chat/completions"),
            | ApiStyle::Anthropic => format!("{base}/messages"),
        }
    }
}

// ============================================================================
// Anthropic constants
// ============================================================================

/// Anthropic API version header value.
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

// ============================================================================
// OpenAI request types
// ============================================================================

#[derive(Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: OpenAiRole,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum OpenAiRole {
    System,
    User,
}

// ============================================================================
// Anthropic request types
// ============================================================================

/// Request body for the Anthropic Messages API.
///
/// Note: unlike OpenAI, the system prompt is a top-level field rather than
/// a message with `role: "system"`.
#[derive(Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    /// Top-level system prompt (not a message).
    system: String,
    messages: Vec<AnthropicMessage>,
    /// Required by Anthropic; no "unlimited" option.
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: AnthropicRole,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum AnthropicRole {
    User,
    #[allow(dead_code)]
    Assistant,
}

// ============================================================================
// Response types (OpenAI)
// ============================================================================

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    message: Option<ResponseMessage>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<ResponseContent>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ResponseContent {
    Text(String),
    Parts(Vec<ResponseContentPart>),
}

#[derive(Deserialize)]
struct ResponseContentPart {
    #[serde(default)]
    text: Option<String>,
}

// ============================================================================
// Response types (Anthropic)
// ============================================================================

/// Anthropic Messages API response.
///
/// The response contains a list of content blocks. We extract text from
/// all `text` type blocks and join them.
#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    /// Present in non-streaming responses. Used by [`response_hit_token_limit`]
    /// via the raw `Value` path rather than through this typed struct.
    #[serde(default)]
    #[allow(dead_code)]
    stop_reason: Option<String>,
}

/// One content block in an Anthropic response.
#[derive(Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    /// Catch-all for unknown block types (tool_use, etc.).
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct AtomizeResponsePayload {
    rewrite: Option<String>,
    #[serde(default)]
    points: Vec<String>,
}

#[derive(Deserialize)]
pub struct AmplifyResponsePayload {
    rewrite: Option<String>,
    #[serde(default)]
    children: Vec<String>,
}

#[derive(Deserialize)]
pub struct DistillResponsePayload {
    reduction: String,
    #[serde(default)]
    redundant_children: Vec<usize>,
}

#[derive(Debug, Clone, Copy, Default)]
struct StreamStats {
    chunk_count: usize,
    char_count: usize,
}

impl StreamStats {
    fn record_chunk(&mut self, chunk: &str) {
        self.chunk_count += 1;
        self.char_count += chunk.chars().count();
    }

    fn has_output(self) -> bool {
        self.char_count > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedStreamEvent {
    Done,
    Delta(String),
    Ignore,
}

/// Incremental SSE decoder for OpenAI-compatible `data:` events.
#[derive(Debug, Default)]
struct SseDataDecoder {
    line_buffer: String,
    event_data_lines: Vec<String>,
}

impl SseDataDecoder {
    fn new() -> Self {
        Self::default()
    }

    fn push_bytes(&mut self, bytes: &[u8]) -> Vec<String> {
        self.line_buffer.push_str(&String::from_utf8_lossy(bytes));
        let mut events = Vec::new();

        while let Some(newline) = self.line_buffer.find('\n') {
            let mut line = self.line_buffer.drain(..=newline).collect::<String>();
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }

            if line.is_empty() {
                if let Some(event) = self.flush_event_data() {
                    events.push(event);
                }
                continue;
            }

            if let Some(data) = line.strip_prefix("data:") {
                self.event_data_lines.push(data.trim_start().to_string());
            }
        }

        events
    }

    fn finish(mut self) -> Vec<String> {
        if !self.line_buffer.is_empty() {
            let mut line = std::mem::take(&mut self.line_buffer);
            if line.ends_with('\r') {
                line.pop();
            }
            if let Some(data) = line.strip_prefix("data:") {
                self.event_data_lines.push(data.trim_start().to_string());
            }
        }

        self.flush_event_data().into_iter().collect()
    }

    fn flush_event_data(&mut self) -> Option<String> {
        if self.event_data_lines.is_empty() {
            return None;
        }
        let payload = self.event_data_lines.join("\n");
        self.event_data_lines.clear();
        Some(payload)
    }
}

fn should_fallback_to_non_stream(err: &LlmError) -> bool {
    match err {
        | LlmError::Api(api) => matches!(api.status.as_u16(), 400 | 404 | 405 | 415 | 422 | 501),
        | LlmError::InvalidResponse => true,
        | _ => false,
    }
}

fn parse_stream_event_payload(payload: &str) -> Result<ParsedStreamEvent, LlmError> {
    let data = payload.trim();
    if data.is_empty() {
        return Ok(ParsedStreamEvent::Ignore);
    }
    // OpenAI termination sentinel.
    if data == "[DONE]" {
        return Ok(ParsedStreamEvent::Done);
    }

    let value: Value = serde_json::from_str(data).map_err(|err| {
        tracing::error!(error = %err, body_preview = %preview_body(data), "invalid streaming json");
        LlmError::InvalidResponse
    })?;

    // Anthropic termination: { "type": "message_stop" }
    if value.get("type").and_then(Value::as_str) == Some("message_stop") {
        return Ok(ParsedStreamEvent::Done);
    }

    if let Some(delta) = extract_stream_delta_from_value(&value) {
        return Ok(ParsedStreamEvent::Delta(delta));
    }

    Ok(ParsedStreamEvent::Ignore)
}

fn extract_stream_delta_from_value(value: &Value) -> Option<String> {
    extract_chat_stream_delta(value)
        .or_else(|| extract_response_api_stream_delta(value))
        .or_else(|| extract_anthropic_stream_delta(value))
        .or_else(|| {
            value
                .get("delta")
                .and_then(extract_stream_content_value)
                .or_else(|| value.get("content").and_then(extract_stream_content_value))
        })
}

fn extract_chat_stream_delta(value: &Value) -> Option<String> {
    value.get("choices").and_then(Value::as_array).and_then(|choices| {
        choices.iter().find_map(|choice| {
            choice
                .get("delta")
                .and_then(extract_stream_content_value)
                .or_else(|| choice.get("text").and_then(extract_stream_content_value))
        })
    })
}

fn extract_response_api_stream_delta(value: &Value) -> Option<String> {
    let event_type = value.get("type").and_then(Value::as_str);
    if event_type == Some("response.output_text.delta") {
        return value.get("delta").and_then(extract_stream_content_value);
    }
    None
}

fn extract_stream_content_value(content: &Value) -> Option<String> {
    match content {
        | Value::String(text) => (!text.is_empty()).then(|| text.to_string()),
        | Value::Array(parts) => {
            let joined = parts.iter().filter_map(extract_stream_text_from_part).collect::<String>();
            (!joined.is_empty()).then_some(joined)
        }
        | Value::Object(_) => extract_stream_text_from_part(content),
        | _ => None,
    }
}

fn extract_stream_text_from_part(part: &Value) -> Option<String> {
    match part {
        | Value::String(text) => (!text.is_empty()).then(|| text.to_string()),
        | Value::Object(obj) => obj
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| obj.get("content").and_then(Value::as_str).map(ToString::to_string))
            .or_else(|| obj.get("value").and_then(Value::as_str).map(ToString::to_string)),
        | _ => None,
    }
}

// ============================================================================
// Response extraction helpers
// ============================================================================

fn extract_completion_content(response: ChatResponse) -> Option<String> {
    response.choices.into_iter().find_map(Choice::extract_content)
}

impl Choice {
    fn extract_content(self) -> Option<String> {
        self.message.and_then(ResponseMessage::into_text).or(self.text).and_then(trim_non_empty)
    }
}

impl ResponseMessage {
    fn into_text(self) -> Option<String> {
        self.content.and_then(ResponseContent::into_text)
    }
}

impl ResponseContent {
    fn into_text(self) -> Option<String> {
        match self {
            | Self::Text(value) => trim_non_empty(value),
            | Self::Parts(parts) => {
                let joined = parts
                    .into_iter()
                    .filter_map(|part| part.text)
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                trim_non_empty(joined)
            }
        }
    }
}

fn trim_non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn preview_body(body: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 512;
    let mut preview = body.chars().take(MAX_PREVIEW_CHARS).collect::<String>();
    if body.chars().count() > MAX_PREVIEW_CHARS {
        preview.push_str("...");
    }
    preview
}

fn extract_completion_content_from_value(value: &Value) -> Option<String> {
    extract_chat_choices_content(value)
        .or_else(|| extract_top_level_content(value))
        .or_else(|| extract_nested_message_content(value))
        .or_else(|| extract_responses_output_content(value))
        .or_else(|| {
            value
                .get("output_text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .and_then(trim_non_empty)
        })
}

fn extract_completion_content_from_chat_value(value: &Value) -> Option<String> {
    serde_json::from_value::<ChatResponse>(value.clone())
        .ok()
        .and_then(extract_completion_content)
        .or_else(|| extract_anthropic_content_from_value(value))
        .or_else(|| extract_completion_content_from_value(value))
}

fn first_choice_finish_reason(value: &Value) -> Option<&str> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
}

fn completion_tokens(value: &Value) -> Option<u64> {
    value
        .get("usage")
        .and_then(Value::as_object)
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
}

fn response_hit_token_limit(value: &Value) -> bool {
    // OpenAI: finish_reason == "length"
    if first_choice_finish_reason(value) == Some("length") {
        return true;
    }
    // Anthropic: stop_reason == "max_tokens"
    if value.get("stop_reason").and_then(Value::as_str) == Some("max_tokens") {
        return true;
    }
    false
}

fn extract_chat_choices_content(value: &Value) -> Option<String> {
    value.get("choices").and_then(Value::as_array).and_then(|choices| {
        choices.iter().find_map(|choice| {
            choice.get("message").and_then(extract_message_content).or_else(|| {
                choice
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .and_then(trim_non_empty)
            })
        })
    })
}

fn extract_top_level_content(value: &Value) -> Option<String> {
    value.get("content").and_then(extract_content_value)
}

fn extract_nested_message_content(value: &Value) -> Option<String> {
    value.get("message").and_then(|message| message.get("content")).and_then(extract_content_value)
}

fn extract_message_content(message: &Value) -> Option<String> {
    message.get("content").and_then(extract_content_value)
}

fn extract_responses_output_content(value: &Value) -> Option<String> {
    value.get("output").and_then(Value::as_array).and_then(|output| {
        output.iter().find_map(|item| item.get("content").and_then(extract_content_value))
    })
}

fn extract_content_value(content: &Value) -> Option<String> {
    match content {
        | Value::String(text) => trim_non_empty(text.clone()),
        | Value::Array(parts) => {
            let joined =
                parts.iter().filter_map(extract_text_from_part).collect::<Vec<_>>().join("\n");
            trim_non_empty(joined)
        }
        | Value::Object(_) => extract_text_from_part(content),
        | _ => None,
    }
}

fn extract_text_from_part(part: &Value) -> Option<String> {
    match part {
        | Value::String(text) => trim_non_empty(text.clone()),
        | Value::Object(obj) => obj
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .and_then(trim_non_empty)
            .or_else(|| {
                obj.get("content")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .and_then(trim_non_empty)
            })
            .or_else(|| {
                obj.get("value")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .and_then(trim_non_empty)
            }),
        | _ => None,
    }
}

// ============================================================================
// Anthropic response extraction
// ============================================================================

/// Extract text content from an Anthropic Messages API response.
///
/// Anthropic returns `{ content: [{ type: "text", text: "..." }, ...] }`.
/// We try structured deserialization first, then fall back to Value-based extraction.
fn extract_anthropic_content_from_value(value: &Value) -> Option<String> {
    // Try structured deserialization.
    if let Ok(response) = serde_json::from_value::<AnthropicResponse>(value.clone()) {
        let text: String = response
            .content
            .into_iter()
            .filter_map(|block| match block {
                | AnthropicContentBlock::Text { text } => {
                    let trimmed = text.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                }
                | AnthropicContentBlock::Other => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        return trim_non_empty(text);
    }

    // Fallback: Anthropic-like shape via Value traversal.
    let content = value.get("content").and_then(Value::as_array)?;
    let text: String = content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    trim_non_empty(text)
}

/// Extract Anthropic streaming delta from an SSE event payload.
///
/// Anthropic streaming events use `event: content_block_delta` with
/// `{ type: "content_block_delta", delta: { type: "text_delta", text: "..." } }`.
fn extract_anthropic_stream_delta(value: &Value) -> Option<String> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    match event_type {
        | "content_block_delta" => value
            .get("delta")
            .and_then(|d| d.get("text"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string),
        | _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AmplifyResponsePayload deserialization tests
    #[test]
    fn amplify_payload_full() {
        let json = r#"{"rewrite": "new text", "children": ["a", "b"]}"#;
        let payload: AmplifyResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, Some("new text".to_string()));
        assert_eq!(payload.children, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn amplify_payload_no_rewrite() {
        let json = r#"{"rewrite": null, "children": ["a"]}"#;
        let payload: AmplifyResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, None);
        assert_eq!(payload.children, vec!["a".to_string()]);
    }

    #[test]
    fn amplify_payload_missing_children() {
        let json = r#"{"rewrite": "r"}"#;
        let payload: AmplifyResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, Some("r".to_string()));
        assert!(payload.children.is_empty());
    }

    #[test]
    fn amplify_payload_empty() {
        let json = r#"{}"#;
        let payload: AmplifyResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, None);
        assert!(payload.children.is_empty());
    }

    // DistillResponsePayload deserialization tests
    #[test]
    fn distill_payload_full() {
        let json = r#"{"reduction": "condensed text", "redundant_children": [0, 2]}"#;
        let payload: DistillResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.reduction, "condensed text");
        assert_eq!(payload.redundant_children, vec![0, 2]);
    }

    #[test]
    fn distill_payload_no_redundant() {
        let json = r#"{"reduction": "text"}"#;
        let payload: DistillResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.reduction, "text");
        assert!(payload.redundant_children.is_empty());
    }

    #[test]
    fn completion_content_reads_message_string() {
        let json = r#"{"choices":[{"message":{"content":"  hello world  "}}]}"#;
        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_completion_content(response).as_deref(), Some("hello world"));
    }

    #[test]
    fn completion_content_reads_message_parts() {
        let json =
            r#"{"choices":[{"message":{"content":[{"text":" first "},{"text":"second"}]}}]}"#;
        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_completion_content(response).as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn completion_content_falls_back_to_choice_text() {
        let json = r#"{"choices":[{"text": "  fallback text  "}]}"#;
        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_completion_content(response).as_deref(), Some("fallback text"));
    }

    #[test]
    fn completion_content_from_top_level_message_shape() {
        let value = serde_json::json!({
            "message": {
                "content": "  from message wrapper  "
            }
        });
        assert_eq!(
            extract_completion_content_from_value(&value).as_deref(),
            Some("from message wrapper")
        );
    }

    #[test]
    fn completion_content_from_responses_output_shape() {
        let value = serde_json::json!({
            "output": [
                {
                    "content": [
                        {"text": " first "},
                        {"content": "second"}
                    ]
                }
            ]
        });
        assert_eq!(extract_completion_content_from_value(&value).as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn response_hit_token_limit_detects_length_finish_reason() {
        let value = serde_json::json!({
            "choices": [
                {
                    "finish_reason": "length"
                }
            ]
        });
        assert!(response_hit_token_limit(&value));
    }

    #[test]
    fn completion_content_from_chat_value_handles_empty_content() {
        let value = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": ""
                    },
                    "finish_reason": "length"
                }
            ],
            "usage": {
                "completion_tokens": 500
            }
        });
        assert!(extract_completion_content_from_chat_value(&value).is_none());
        assert_eq!(completion_tokens(&value), Some(500));
        assert_eq!(first_choice_finish_reason(&value), Some("length"));
    }

    #[test]
    fn stream_delta_extracts_chat_completion_delta_content() {
        let value = serde_json::json!({
            "choices": [
                {
                    "delta": {
                        "content": "hello"
                    }
                }
            ]
        });
        assert_eq!(extract_stream_delta_from_value(&value).as_deref(), Some("hello"));
    }

    #[test]
    fn stream_delta_extracts_responses_api_delta_content() {
        let value = serde_json::json!({
            "type": "response.output_text.delta",
            "delta": "streaming"
        });
        assert_eq!(extract_stream_delta_from_value(&value).as_deref(), Some("streaming"));
    }

    #[test]
    fn sse_decoder_joins_multiline_data_and_done_event() {
        let mut decoder = SseDataDecoder::new();
        let first = decoder.push_bytes(b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"");
        assert!(first.is_empty());
        let second = decoder.push_bytes(b"}}]}\n\ndata: [DONE]\n\n");
        assert_eq!(second.len(), 2);
        assert!(matches!(parse_stream_event_payload(&second[1]), Ok(ParsedStreamEvent::Done)));
    }
}
