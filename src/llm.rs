//! LLM integration: configuration, prompt construction, and API client.
//!
//! The client speaks the OpenAI-compatible chat completions API. Configuration
//! is loaded from environment variables (`LLM_BASE_URL`, `LLM_API_KEY`,
//! `LLM_MODEL`) with fallback to a TOML config file.

use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf};
use thiserror::Error;

/// Validated LLM endpoint configuration.
///
/// Invariants (enforced by [`LlmConfig::load`]):
/// - `base_url` starts with `https://`
/// - `api_key` and `model` are non-empty after trimming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    base_url: String,
    api_key: String,
    model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.example.com/v1".to_string(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

impl LlmConfig {
    /// Load config from env vars with TOML file fallback.
    ///
    /// Returns `Err` if any required field is missing or invalid.
    pub fn load() -> Result<Self, LlmConfigError> {
        Self::from_env_or_file()
    }

    fn from_file() -> Result<Option<Self>, LlmConfigError> {
        let Some(path) = Self::config_path() else {
            return Ok(None);
        };
        match fs::read_to_string(&path) {
            | Ok(contents) => toml::from_str(&contents)
                .map(Some)
                .map_err(|err| ConfigFileError::parse(path.clone(), err).into()),
            | Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|err| {
                        LlmConfigError::from(ConfigFileError::create_dir(parent.to_path_buf(), err))
                    })?;
                }
                fs::write(&path, Self::default_template()).map_err(|err| {
                    LlmConfigError::from(ConfigFileError::write(path.clone(), err))
                })?;
                Ok(None)
            }
            | Err(err) => Err(LlmConfigError::from(ConfigFileError::read(path.clone(), err))),
        }
    }

    fn from_env_or_file() -> Result<Self, LlmConfigError> {
        fn retrieve_non_empty_env_var(var_name: &str) -> Option<String> {
            match env::var(var_name) {
                | Err(_) => None,
                | Ok(value) if value.is_empty() => None,
                | Ok(value) => Some(value),
            }
        }

        let mut base_url = retrieve_non_empty_env_var("LLM_BASE_URL");
        let mut api_key = retrieve_non_empty_env_var("LLM_API_KEY");
        let mut model = retrieve_non_empty_env_var("LLM_MODEL");

        if let Some(file_config) = Self::from_file()? {
            if base_url.is_none() {
                base_url = Some(file_config.base_url);
            }
            if api_key.is_none() {
                api_key = Some(file_config.api_key);
            }
            if model.is_none() {
                model = Some(file_config.model);
            }
        }

        let Some(base_url) = base_url else {
            return Err(LlmConfigError::MissingConfig);
        };
        let Some(api_key) = api_key else {
            return Err(LlmConfigError::MissingConfig);
        };
        let Some(model) = model else {
            return Err(LlmConfigError::MissingConfig);
        };

        let base_url = base_url.trim().to_string();
        let api_key = api_key.trim().to_string();
        let model = model.trim().to_string();

        if !base_url.starts_with("https://") {
            return Err(LlmConfigError::InvalidConfig(InvalidConfigReason::BaseUrlNotHttps));
        }
        if api_key.is_empty() {
            return Err(LlmConfigError::InvalidConfig(InvalidConfigReason::ApiKeyEmpty));
        }
        if model.is_empty() {
            return Err(LlmConfigError::InvalidConfig(InvalidConfigReason::ModelEmpty));
        }

        Ok(Self { base_url, api_key, model })
    }

    fn config_path() -> Option<PathBuf> {
        AppPaths::llm_config()
    }

    fn default_template() -> String {
        let mut rendered = String::from("# LLM config\n");
        let body =
            toml::to_string_pretty(&Self::default()).expect("failed to render default config");
        rendered.push_str(&body);
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LlmConfigError {
    #[error("missing LLM config (env vars or config file)")]
    MissingConfig,
    #[error("invalid LLM config: {0}")]
    InvalidConfig(InvalidConfigReason),
    #[error("LLM config file error: {0}")]
    ConfigFile(ConfigFileError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InvalidConfigReason {
    #[error("LLM_BASE_URL must start with https://")]
    BaseUrlNotHttps,
    #[error("LLM_API_KEY is empty")]
    ApiKeyEmpty,
    #[error("LLM_MODEL is empty")]
    ModelEmpty,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("invalid request")]
    InvalidRequest,
    #[error(transparent)]
    Config(#[from] LlmConfigError),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid response")]
    InvalidResponse,
    #[error("invalid expand response")]
    InvalidExpandResponse,
    #[error("invalid reduce response")]
    InvalidReduceResponse,
}

/// One candidate child point returned from an expand request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandSuggestion {
    point: String,
}

impl ExpandSuggestion {
    /// Construct one suggestion with raw point text.
    pub fn new(point: String) -> Self {
        Self { point }
    }

    /// Consume and return the suggestion text.
    pub fn into_point(self) -> String {
        self.point
    }
}

/// Structured result returned by one expand request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandResult {
    rewrite: Option<String>,
    children: Vec<ExpandSuggestion>,
}

impl ExpandResult {
    /// Build an expand result from optional rewrite and children.
    pub fn new(rewrite: Option<String>, children: Vec<ExpandSuggestion>) -> Self {
        Self { rewrite, children }
    }

    /// Consume the result and return owned parts.
    pub fn into_parts(self) -> (Option<String>, Vec<ExpandSuggestion>) {
        (self.rewrite, self.children)
    }
}

/// Structured result returned by one reduce request.
///
/// Contains the condensed text plus 0-based indices of existing children
/// the LLM considers redundant (their content is captured by the reduction).
/// The caller maps these indices to `BlockId`s using the children snapshot
/// that was active at request time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReduceResult {
    reduction: String,
    /// 0-based indices into the `existing_children` that were sent in the prompt.
    redundant_children: Vec<usize>,
}

impl ReduceResult {
    pub fn new(reduction: String, redundant_children: Vec<usize>) -> Self {
        Self { reduction, redundant_children }
    }

    /// Consume and return owned parts.
    pub fn into_parts(self) -> (String, Vec<usize>) {
        (self.reduction, self.redundant_children)
    }
}

/// Immutable snapshot of a block's LLM-relevant context: ancestor lineage,
/// existing child point texts, and user-selected friend blocks.
///
/// Constructed by the store layer; consumed by [`LlmClient`] methods.
/// The `existing_children` field carries only point texts (no `BlockId`s)
/// so this module stays decoupled from store identity types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockContext {
    lineage: Lineage,
    existing_children: Vec<String>,
    friend_blocks: Vec<FriendContext>,
}

/// One friend context item supplied alongside lineage and existing children.
///
/// `point` is the friend block text itself.
/// `perspective` is optional target-authored framing describing how the
/// current block views that friend block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendContext {
    /// Friend block text included in prompt context.
    point: String,
    /// Optional framing lens for interpreting this friend block.
    perspective: Option<String>,
}

impl BlockContext {
    pub fn new(
        lineage: Lineage, existing_children: Vec<String>, friend_blocks: Vec<FriendContext>,
    ) -> Self {
        Self { lineage, existing_children, friend_blocks }
    }

    pub fn lineage(&self) -> &Lineage {
        &self.lineage
    }

    pub fn existing_children(&self) -> &[String] {
        &self.existing_children
    }

    pub fn friend_blocks(&self) -> &[FriendContext] {
        &self.friend_blocks
    }

    fn is_empty(&self) -> bool {
        self.lineage.is_empty()
    }
}

impl FriendContext {
    pub fn new(point: String, perspective: Option<String>) -> Self {
        Self { point, perspective }
    }

    pub fn point(&self) -> &str {
        &self.point
    }

    pub fn perspective(&self) -> Option<&str> {
        self.perspective.as_deref()
    }
}

/// Structured API error details returned by the upstream LLM endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("api error: status {status}: {body}")]
pub struct ApiError {
    /// HTTP status returned by the LLM endpoint.
    status: reqwest::StatusCode,
    /// Raw response body to help diagnose request failures.
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{kind} ({path})")]
pub struct ConfigFileError {
    path: PathBuf,
    kind: ConfigFileErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigFileErrorKind {
    #[error("failed to read config file: {0:?}")]
    Read(io::ErrorKind),
    #[error("failed to create config directory: {0:?}")]
    CreateDir(io::ErrorKind),
    #[error("failed to create config file: {0:?}")]
    Write(io::ErrorKind),
    #[error("failed to parse config file: {0}")]
    Parse(String),
}

impl ConfigFileError {
    fn read(path: PathBuf, err: io::Error) -> Self {
        Self { path, kind: ConfigFileErrorKind::Read(err.kind()) }
    }

    fn create_dir(path: PathBuf, err: io::Error) -> Self {
        Self { path, kind: ConfigFileErrorKind::CreateDir(err.kind()) }
    }

    fn write(path: PathBuf, err: io::Error) -> Self {
        Self { path, kind: ConfigFileErrorKind::Write(err.kind()) }
    }

    fn parse(path: PathBuf, err: toml::de::Error) -> Self {
        Self { path, kind: ConfigFileErrorKind::Parse(err.to_string()) }
    }
}

impl From<ConfigFileError> for LlmConfigError {
    fn from(err: ConfigFileError) -> Self {
        Self::ConfigFile(err)
    }
}

/// Ordered ancestor chain from root to a target block.
///
/// Used to give the LLM context about where in the document tree the
/// target point lives. The last item is always the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lineage {
    items: Vec<LineageItem>,
}

impl Lineage {
    pub fn new(items: Vec<LineageItem>) -> Self {
        Self { items }
    }

    pub fn from_points(points: Vec<String>) -> Self {
        Self::new(points.into_iter().map(LineageItem::new).collect())
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = &LineageItem> {
        self.items.iter()
    }

    pub fn points(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(LineageItem::point)
    }
}

/// One element in a [`Lineage`] chain: wraps a block's point text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageItem {
    point: String,
}

impl LineageItem {
    pub fn new(point: String) -> Self {
        Self { point }
    }

    fn point(&self) -> &str {
        &self.point
    }
}

/// HTTP client for the OpenAI-compatible chat completions endpoint.
///
/// Stateless aside from config; safe to construct per-request.
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self { config, http: reqwest::Client::new() }
    }

    /// Reduce a block's point using ancestor lineage and existing children as context.
    ///
    /// When existing children are present, the LLM may identify some as
    /// redundant (their content is subsumed by the reduction). Defensive
    /// parsing: if the response is not valid JSON, it is treated as a
    /// plain-text reduction with no redundant children.
    pub async fn reduce_block(
        &self, context: &BlockContext, instruction: Option<&str>,
    ) -> Result<ReduceResult, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let has_children =
            !context.existing_children.is_empty() || !context.friend_blocks.is_empty();
        let prompt = Prompt::reduce_from_context(context, instruction);
        let max_tokens = if has_children { 400 } else { 200 };
        let content = self.request_completion("reduce", prompt, 0.2, max_tokens).await?;

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
    pub async fn expand_block(
        &self, context: &BlockContext, instruction: Option<&str>,
    ) -> Result<ExpandResult, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::expand_from_context(context, instruction);
        let content = self.request_completion("expand", prompt, 0.7, 500).await?;
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
    pub async fn inquire(&self, context: &BlockContext, instruction: &str) -> Result<String, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }
        if instruction.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::inquire_from_context(context, instruction);
        let content = self.request_completion("inquire", prompt, 0.7, 500).await?;

        tracing::info!(chars = content.len(), "llm inquire response");
        Ok(content.trim().to_string())
    }

    async fn request_completion(
        &self, purpose: &'static str, prompt: Prompt, temperature: f32, max_completion_tokens: u32,
    ) -> Result<String, LlmError> {
        let url = self.chat_url();
        tracing::info!(model = %self.config.model, url = %url, purpose, "llm request");
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message { role: Role::System, content: prompt.system },
                Message { role: Role::User, content: prompt.user },
            ],
            temperature,
            max_completion_tokens,
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
        let response: ChatResponse =
            serde_json::from_str(&body).map_err(|_| LlmError::InvalidResponse)?;

        let content = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content.trim().to_string())
            .filter(|content| !content.is_empty())
            .ok_or(LlmError::InvalidResponse)?;

        tracing::info!(purpose, chars = content.len(), "llm completion response");
        Ok(content)
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'))
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_completion_tokens: u32,
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
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

/// System + user prompt pair sent to the chat completions endpoint.
struct Prompt {
    system: String,
    user: String,
}

impl Prompt {
    /// Format lineage items as labeled lines.
    fn format_lineage_lines(lineage: &Lineage) -> String {
        let mut lines = String::new();
        let total = lineage.items.len();
        for (index, item) in lineage.iter().enumerate() {
            let label = if index + 1 == total { "Target" } else { "Parent" };
            lines.push_str(&format!("{label}: {}\n", item.point()));
        }
        lines
    }

    /// Format existing children as indexed lines.
    fn format_children_lines(children: &[String]) -> String {
        let mut lines = String::new();
        for (index, child) in children.iter().enumerate() {
            lines.push_str(&format!("[{index}] {child}\n"));
        }
        lines
    }

    fn format_friend_blocks_lines(friend_blocks: &[FriendContext]) -> String {
        let mut lines = String::new();
        for (index, friend_block) in friend_blocks.iter().enumerate() {
            if let Some(perspective) = friend_block.perspective() {
                lines.push_str(&format!(
                    "[{index}] {} (perspective: {perspective})\n",
                    friend_block.point()
                ));
            } else {
                lines.push_str(&format!("[{index}] {}\n", friend_block.point()));
            }
        }
        lines
    }

    fn reduce_from_context(context: &BlockContext, instruction: Option<&str>) -> Self {
        let lineage_lines = Self::format_lineage_lines(&context.lineage);
        let friend_lines = Self::format_friend_blocks_lines(&context.friend_blocks);

        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();

        if context.existing_children.is_empty() && context.friend_blocks.is_empty() {
            return Self {
                system: format!("{}You reduce a bullet point using its ancestors as context. Return strict JSON only: {{\"reduction\": string}}. The reduction must be a single concise sentence. No markdown, no extra keys.", instruction_prefix),
                user: format!("Reduce the target point with context:\n{lineage_lines}"),
            };
        }

        if context.existing_children.is_empty() {
            return Self {
                system: format!("{}You reduce a bullet point using its ancestors plus friend blocks as context. Return strict JSON only: {{\"reduction\": string}}. The reduction must be a single concise sentence. Friend blocks are user-selected related context and are not children of the target. Each friend block may include an optional perspective describing how the target views that friend block; use it when helpful. No markdown, no extra keys.", instruction_prefix),
                user: format!(
                    "Reduce the target point with context:\n{lineage_lines}\nFriend blocks:\n{friend_lines}"
                ),
            };
        }

        let children_lines = Self::format_children_lines(&context.existing_children);
        let friend_context = if context.friend_blocks.is_empty() {
            String::new()
        } else {
            format!("\nFriend blocks:\n{friend_lines}")
        };
        Self {
            system: format!("{}You reduce a bullet point using its ancestors, existing children, and optional friend blocks as context. Return strict JSON only: {{\"reduction\": string, \"redundant_children\": number[]}}. The reduction must be a single concise sentence that captures the essential meaning. redundant_children: 0-based indices of existing children whose information is fully captured by the reduction and can be safely removed. Friend blocks are additional context only and must never appear in redundant_children. Friend blocks may include optional perspective text that can refine interpretation. Only mark a child redundant when its content is genuinely subsumed. No markdown, no extra keys.", instruction_prefix),
            user: format!(
                "Reduce the target point with context:\n{lineage_lines}\nExisting children:\n{children_lines}{friend_context}"
            ),
        }
    }

    fn expand_from_context(context: &BlockContext, instruction: Option<&str>) -> Self {
        let lineage_lines = Self::format_lineage_lines(&context.lineage);
        let friend_lines = Self::format_friend_blocks_lines(&context.friend_blocks);

        let instruction_prefix = instruction.map(|i| format!("{}\n\n", i)).unwrap_or_default();

        if context.existing_children.is_empty() && context.friend_blocks.is_empty() {
            return Self {
                system: format!("{}You expand one target bullet point using its ancestors as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite. No markdown, no extra keys.", instruction_prefix),
                user: format!("Expand the target point with context:\n{lineage_lines}"),
            };
        }

        if context.existing_children.is_empty() {
            return Self {
                system: format!("{}You expand one target bullet point using its ancestors plus friend blocks as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite. Friend blocks are user-selected related context and are not children of the target. Friend blocks may include an optional perspective describing how the target views that friend block; use it when relevant. No markdown, no extra keys.", instruction_prefix),
                user: format!(
                    "Expand the target point with context:\n{lineage_lines}\nFriend blocks:\n{friend_lines}"
                ),
            };
        }

        let children_lines = Self::format_children_lines(&context.existing_children);
        let friend_context = if context.friend_blocks.is_empty() {
            String::new()
        } else {
            format!("\nFriend blocks:\n{friend_lines}")
        };
        Self {
            system: format!("{}You expand one target bullet point using its ancestors, existing children, and optional friend blocks as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise NEW child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, should not restate the rewrite, and MUST NOT overlap with the existing children listed below. Friend blocks are additional context only and are not children. Friend blocks may include optional perspective text that can refine interpretation. No markdown, no extra keys.", instruction_prefix),
            user: format!(
                "Expand the target point with context:\n{lineage_lines}\nExisting children:\n{children_lines}{friend_context}"
            ),
        }
    }

    /// Build a prompt for a one-time instruction inquiry.
    ///
    /// The inquiry prompt includes the block's lineage and friend blocks as context,
    /// followed by the user's instruction. The response is a free-form text answer
    /// that can be applied as a rewrite to the block's point.
    fn inquire_from_context(context: &BlockContext, instruction: &str) -> Self {
        let lineage_lines = Self::format_lineage_lines(&context.lineage);
        let friend_lines = Self::format_friend_blocks_lines(&context.friend_blocks);

        let friend_context = if context.friend_blocks.is_empty() {
            String::new()
        } else {
            format!("\nFriend blocks:\n{friend_lines}")
        };

        Self {
            system: "You are a helpful writing assistant. Respond to the user's instruction based on the provided context.".to_string(),
            user: format!(
                "Context:\n{lineage_lines}{friend_context}\n\nInstruction: {instruction}\n\nProvide a response that addresses the instruction."
            ),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ExpandResponsePayload {
    rewrite: Option<String>,
    #[serde(default)]
    children: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReduceResponsePayload {
    reduction: String,
    #[serde(default)]
    redundant_children: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ExpandSuggestion tests
    #[test]
    fn expand_suggestion_into_point() {
        let suggestion = ExpandSuggestion::new("text".into());
        assert_eq!(suggestion.into_point(), "text");
    }

    // ExpandResult tests
    #[test]
    fn expand_result_into_parts_with_both() {
        let suggestion = ExpandSuggestion::new("child".into());
        let result = ExpandResult::new(Some("rewrite".into()), vec![suggestion]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, Some("rewrite".to_string()));
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], ExpandSuggestion::new("child".into()));
    }

    #[test]
    fn expand_result_into_parts_rewrite_only() {
        let result = ExpandResult::new(Some("rewrite".into()), vec![]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, Some("rewrite".to_string()));
        assert!(children.is_empty());
    }

    #[test]
    fn expand_result_into_parts_children_only() {
        let suggestion1 = ExpandSuggestion::new("child1".into());
        let suggestion2 = ExpandSuggestion::new("child2".into());
        let result = ExpandResult::new(None, vec![suggestion1, suggestion2]);
        let (rewrite, children) = result.into_parts();
        assert_eq!(rewrite, None);
        assert_eq!(children.len(), 2);
    }

    // ReduceResult tests
    #[test]
    fn reduce_result_into_parts() {
        let result = ReduceResult::new("condensed".into(), vec![0, 2]);
        let (reduction, redundant) = result.into_parts();
        assert_eq!(reduction, "condensed");
        assert_eq!(redundant, vec![0, 2]);
    }

    #[test]
    fn reduce_result_empty_redundant() {
        let result = ReduceResult::new("text".into(), vec![]);
        let (_, redundant) = result.into_parts();
        assert!(redundant.is_empty());
    }

    // Lineage tests
    #[test]
    fn lineage_from_points_creates_items() {
        let lineage = Lineage::from_points(vec!["a".into(), "b".into()]);
        let expected =
            Lineage::new(vec![LineageItem::new("a".into()), LineageItem::new("b".into())]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_empty() {
        let lineage = Lineage::from_points(vec![]);
        let expected = Lineage::new(vec![]);
        assert_eq!(lineage, expected);
    }

    #[test]
    fn lineage_from_points_roundtrip() {
        let lineage = Lineage::from_points(vec!["a".into()]);
        let expected = Lineage::new(vec![LineageItem::new("a".into())]);
        assert_eq!(lineage, expected);
    }

    // BlockContext tests
    #[test]
    fn block_context_empty_lineage_is_empty() {
        let ctx = BlockContext::new(Lineage::from_points(vec![]), vec![], vec![]);
        assert!(ctx.is_empty());
    }

    #[test]
    fn block_context_with_lineage_is_not_empty() {
        let ctx = BlockContext::new(Lineage::from_points(vec!["root".into()]), vec![], vec![]);
        assert!(!ctx.is_empty());
    }

    #[test]
    fn block_context_accessors() {
        let lineage = Lineage::from_points(vec!["root".into()]);
        let children = vec!["child_a".to_string(), "child_b".to_string()];
        let friends = vec![FriendContext::new("friend".to_string(), Some("ally".to_string()))];
        let ctx = BlockContext::new(lineage.clone(), children.clone(), friends.clone());
        assert_eq!(ctx.lineage(), &lineage);
        assert_eq!(ctx.existing_children(), &children[..]);
        assert_eq!(ctx.friend_blocks(), &friends[..]);
    }

    // LlmConfigError Display tests
    #[test]
    fn config_error_missing_display() {
        let err = LlmConfigError::MissingConfig;
        let msg = err.to_string();
        assert!(msg.contains("missing"));
    }

    #[test]
    fn config_error_invalid_display() {
        let err = LlmConfigError::InvalidConfig(InvalidConfigReason::ApiKeyEmpty);
        let msg = err.to_string();
        assert!(msg.contains("empty"));
    }

    // Prompt formatting tests
    #[test]
    fn reduce_prompt_labels_target_last() {
        let lineage = Lineage::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&context, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_labels_target_last() {
        let lineage = Lineage::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&context, None);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_mentions_concise_and_non_overlapping_constraints() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let context = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&context, None);
        assert!(prompt.system.contains("one concise sentence"));
        assert!(prompt.system.contains("mutually non-overlapping"));
        assert!(prompt.system.contains("distinct subtopic"));
        assert!(prompt.system.contains("should not restate the rewrite"));
    }

    // Prompt tests for children context
    #[test]
    fn expand_prompt_includes_existing_children() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["existing child A".to_string(), "existing child B".to_string()];
        let ctx = BlockContext::new(lineage, children, vec![]);
        let prompt = Prompt::expand_from_context(&ctx, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] existing child A"));
        assert!(prompt.user.contains("[1] existing child B"));
        assert!(prompt.system.contains("MUST NOT overlap with the existing children"));
    }

    #[test]
    fn expand_prompt_without_children_omits_section() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::expand_from_context(&ctx, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn reduce_prompt_includes_existing_children() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let children = vec!["child A".to_string()];
        let ctx = BlockContext::new(lineage, children, vec![]);
        let prompt = Prompt::reduce_from_context(&ctx, None);
        assert!(prompt.user.contains("Existing children:"));
        assert!(prompt.user.contains("[0] child A"));
        assert!(prompt.system.contains("redundant_children"));
    }

    #[test]
    fn reduce_prompt_without_children_is_plain() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let ctx = BlockContext::new(lineage, vec![], vec![]);
        let prompt = Prompt::reduce_from_context(&ctx, None);
        assert!(!prompt.user.contains("Existing children:"));
    }

    #[test]
    fn expand_prompt_includes_friend_blocks() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let friends = vec![
            FriendContext::new("peer concept A".to_string(), Some("historical lens".to_string())),
            FriendContext::new("peer concept B".to_string(), None),
        ];
        let ctx = BlockContext::new(lineage, vec![], friends);
        let prompt = Prompt::expand_from_context(&ctx, None);
        assert!(prompt.user.contains("Friend blocks:"));
        assert!(prompt.user.contains("[0] peer concept A (perspective: historical lens)"));
        assert!(prompt.user.contains("[1] peer concept B"));
    }

    #[test]
    fn reduce_prompt_includes_friend_blocks() {
        let lineage = Lineage::from_points(vec!["root".into(), "target".into()]);
        let friends = vec![FriendContext::new(
            "supporting external detail".to_string(),
            Some("skeptical counterpoint".to_string()),
        )];
        let ctx = BlockContext::new(lineage, vec![], friends);
        let prompt = Prompt::reduce_from_context(&ctx, None);
        assert!(prompt.user.contains("Friend blocks:"));
        assert!(
            prompt
                .user
                .contains("[0] supporting external detail (perspective: skeptical counterpoint)")
        );
    }

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
}
