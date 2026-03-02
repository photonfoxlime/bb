//! LLM HTTP client and request/response types.
//! The client speaks the OpenAI-compatible chat completions API.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use crate::llm::config::LlmConfig;
use crate::llm::context::{BlockContext, ExpandResult, ExpandSuggestion, ReduceResult};
use crate::llm::error::{ApiError, LlmError};
use crate::llm::prompt::Prompt;
use iced::futures::SinkExt;
use iced::futures::Stream;
use iced::futures::StreamExt;

/// Incremental inquiry stream event emitted by [`LlmClient::inquire_stream`].
#[derive(Debug)]
pub enum InquireStreamEvent {
    /// One response delta chunk from the model.
    Chunk(String),
    /// Non-recoverable error while producing inquiry chunks.
    Failed(LlmError),
    /// Terminal event emitted exactly once when the request ends.
    Finished,
}

/// HTTP client for the OpenAI-compatible chat completions endpoint.
///
/// # Invariants
/// - The client is stateless aside from the config.
/// - Safe to construct per-request or share across requests.
///
/// # Example
/// ```ignore
/// let config = LlmConfig::from_raw(base_url, api_key, model)?;
/// let client = LlmClient::new(config);
/// let result = client.expand_block(&context, None, Some(500)).await?;
/// ```
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    /// Create a new LLM client with the given configuration.
    pub fn new(config: LlmConfig) -> Self {
        Self { config, http: reqwest::Client::new() }
    }

    /// Reduce a block's point using ancestor lineage and existing children as context.
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
    /// - Returns `Ok(ReduceResult)` with the condensed text and indices of redundant children.
    /// - If response parsing fails, falls back to plain-text reduction with no redundant children.
    pub async fn reduce_block(
        &self, context: &BlockContext, instruction: Option<&str>, max_tokens: Option<u32>,
    ) -> Result<ReduceResult, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let has_children =
            !context.existing_children.is_empty() || !context.friend_blocks.is_empty();
        let prompt = Prompt::reduce_from_context(context, instruction);
        let content = self.request_completion("reduce", &prompt, 0.2, max_tokens).await?;

        if has_children {
            // Try structured JSON first; fall back to plain-text reduction.
            if let Ok(payload) = serde_json::from_str::<ReduceResponsePayload>(&content) {
                let reduction = payload.reduction.trim().to_string();
                if reduction.is_empty() {
                    return Err(LlmError::InvalidReduceResponse);
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
                return Ok(ReduceResult::new(reduction, redundant_children));
            }
        }

        // Plain-text fallback (no children or JSON parse failed).
        let reduction = content.trim().to_string();
        if reduction.is_empty() {
            return Err(LlmError::InvalidReduceResponse);
        }
        tracing::info!(chars = reduction.len(), "llm reduce response (plain)");
        Ok(ReduceResult::new(reduction, vec![]))
    }

    /// Expand one target point into rewrite and concise child point candidates.
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
    /// - Returns `Ok(ExpandResult)` with optional rewrite and child suggestions.
    /// - Returns `Err(LlmError::InvalidExpandResponse)` if the response cannot be parsed.
    pub async fn expand_block(
        &self, context: &BlockContext, instruction: Option<&str>, max_tokens: Option<u32>,
    ) -> Result<ExpandResult, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::expand_from_context(context, instruction);
        let content = self.request_completion("expand", &prompt, 0.7, max_tokens).await?;
        let payload: ExpandResponsePayload =
            serde_json::from_str(&content).map_err(|_| LlmError::InvalidExpandResponse)?;

        let rewrite =
            payload.rewrite.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
        let children = payload
            .children
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(ExpandSuggestion::new)
            .collect::<Vec<_>>();

        if rewrite.is_none() && children.is_empty() {
            return Err(LlmError::InvalidExpandResponse);
        }

        tracing::info!(
            rewrite = rewrite.is_some(),
            children = children.len(),
            "llm expand response"
        );
        Ok(ExpandResult::new(rewrite, children))
    }

    /// Send an instruction as a one-time inquiry to the LLM.
    ///
    /// The instruction is sent as a user message with the block context.
    /// Returns a one-time response that can be applied as a rewrite.
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
    ) -> Result<String, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }
        if instruction.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::inquire_from_context(context, instruction);
        let content = self.request_completion("inquire", &prompt, 0.7, max_tokens).await?;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Err(LlmError::InvalidResponse);
        }

        tracing::info!(chars = trimmed.len(), "llm inquire response");
        Ok(trimmed.to_string())
    }

    /// Stream inquiry response chunks as they are produced by the model.
    ///
    /// The method prefers true server-sent-event streaming (`stream: true`). If
    /// the provider does not support streaming and no chunks were emitted, it
    /// falls back to a one-shot inquiry request and emits that full response as
    /// one [`InquireStreamEvent::Chunk`].
    ///
    /// `max_tokens` caps the completion length (`None` = unlimited, omits the
    /// field from the API request).
    ///
    /// # Requires
    /// - `context` must not be empty (must have a lineage).
    /// - `instruction` must not be empty.
    ///
    /// # Ensures
    /// - Emits zero or more [`InquireStreamEvent::Chunk`] events.
    /// - Emits exactly one terminal [`InquireStreamEvent::Finished`] event.
    /// - Emits [`InquireStreamEvent::Failed`] before `Finished` on failure.
    pub fn inquire_stream(
        self, context: BlockContext, instruction: String, timeout: Duration,
        max_tokens: Option<u32>,
    ) -> impl Stream<Item = InquireStreamEvent> {
        iced::stream::channel(64, async move |mut output| {
            let request = async {
                if context.is_empty() || instruction.is_empty() {
                    return Err(LlmError::InvalidRequest);
                }

                let prompt = Prompt::inquire_from_context(&context, &instruction);

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
                        self.emit_inquiry_fallback(&context, &instruction, &mut output, max_tokens)
                            .await
                    }
                    | Err(err) if should_fallback_to_non_stream(&err) => {
                        tracing::warn!(
                            error = %err,
                            "llm inquire streaming unsupported; retrying with one-shot request"
                        );
                        self.emit_inquiry_fallback(&context, &instruction, &mut output, max_tokens)
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
                let _ = output.send(InquireStreamEvent::Failed(err)).await;
            }

            let _ = output.send(InquireStreamEvent::Finished).await;
        })
    }

    async fn request_completion(
        &self, purpose: &'static str, prompt: &Prompt, temperature: f32,
        max_completion_tokens: Option<u32>,
    ) -> Result<String, LlmError> {
        let url = self.chat_url();
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
        output: &mut iced::futures::channel::mpsc::Sender<InquireStreamEvent>,
        max_tokens: Option<u32>,
    ) -> Result<(), LlmError> {
        let content = self.inquire(context, instruction, max_tokens).await?;
        let _ = output.send(InquireStreamEvent::Chunk(content.clone())).await;
        tracing::info!(chars = content.len(), "llm inquire fallback response");
        Ok(())
    }

    async fn stream_inquiry_chunks(
        &self, prompt: &Prompt,
        output: &mut iced::futures::channel::mpsc::Sender<InquireStreamEvent>,
        max_tokens: Option<u32>,
    ) -> Result<StreamStats, LlmError> {
        let url = self.chat_url();
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
                        if output.send(InquireStreamEvent::Chunk(delta)).await.is_err() {
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
                        if output.send(InquireStreamEvent::Chunk(delta)).await.is_err() {
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
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message { role: Role::System, content: prompt.system.clone() },
                Message { role: Role::User, content: prompt.user.clone() },
            ],
            temperature,
            max_completion_tokens,
            stream: None,
        };

        let response = self
            .http
            .post(url)
            .bearer_auth(self.config.api_key.clone())
            .json(&request)
            .send()
            .await?;
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
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message { role: Role::System, content: prompt.system.clone() },
                Message { role: Role::User, content: prompt.user.clone() },
            ],
            temperature,
            max_completion_tokens,
            stream: Some(true),
        };

        let response = self
            .http
            .post(url)
            .bearer_auth(self.config.api_key.clone())
            .json(&request)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError { status, body }.into());
        }
        Ok(response)
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'))
    }
}

// ============================================================================
// Request types
// ============================================================================

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct Message {
    role: Role,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    System,
    User,
}

// ============================================================================
// Response types
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

#[derive(Deserialize)]
pub struct ExpandResponsePayload {
    rewrite: Option<String>,
    #[serde(default)]
    children: Vec<String>,
}

#[derive(Deserialize)]
pub struct ReduceResponsePayload {
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
    if data == "[DONE]" {
        return Ok(ParsedStreamEvent::Done);
    }

    let value: Value = serde_json::from_str(data).map_err(|err| {
        tracing::error!(error = %err, body_preview = %preview_body(data), "invalid streaming json");
        LlmError::InvalidResponse
    })?;

    if let Some(delta) = extract_stream_delta_from_value(&value) {
        return Ok(ParsedStreamEvent::Delta(delta));
    }

    Ok(ParsedStreamEvent::Ignore)
}

fn extract_stream_delta_from_value(value: &Value) -> Option<String> {
    extract_chat_stream_delta(value).or_else(|| extract_response_api_stream_delta(value)).or_else(
        || {
            value
                .get("delta")
                .and_then(extract_stream_content_value)
                .or_else(|| value.get("content").and_then(extract_stream_content_value))
        },
    )
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
    first_choice_finish_reason(value) == Some("length")
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

#[cfg(test)]
mod tests {
    use super::*;

    // ExpandResponsePayload deserialization tests
    #[test]
    fn expand_payload_full() {
        let json = r#"{"rewrite": "new text", "children": ["a", "b"]}"#;
        let payload: ExpandResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, Some("new text".to_string()));
        assert_eq!(payload.children, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn expand_payload_no_rewrite() {
        let json = r#"{"rewrite": null, "children": ["a"]}"#;
        let payload: ExpandResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, None);
        assert_eq!(payload.children, vec!["a".to_string()]);
    }

    #[test]
    fn expand_payload_missing_children() {
        let json = r#"{"rewrite": "r"}"#;
        let payload: ExpandResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, Some("r".to_string()));
        assert!(payload.children.is_empty());
    }

    #[test]
    fn expand_payload_empty() {
        let json = r#"{}"#;
        let payload: ExpandResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.rewrite, None);
        assert!(payload.children.is_empty());
    }

    // ReduceResponsePayload deserialization tests
    #[test]
    fn reduce_payload_full() {
        let json = r#"{"reduction": "condensed text", "redundant_children": [0, 2]}"#;
        let payload: ReduceResponsePayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.reduction, "condensed text");
        assert_eq!(payload.redundant_children, vec![0, 2]);
    }

    #[test]
    fn reduce_payload_no_redundant() {
        let json = r#"{"reduction": "text"}"#;
        let payload: ReduceResponsePayload = serde_json::from_str(json).unwrap();
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
