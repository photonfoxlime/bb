//! LLM integration: configuration, prompt construction, and API client.
//!
//! The client speaks the OpenAI-compatible chat completions API. Configuration
//! is loaded from environment variables (`LLM_BASE_URL`, `LLM_API_KEY`,
//! `LLM_MODEL`) with fallback to a TOML config file.

use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf};
use thiserror::Error;
use tracing;

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

    /// Reduce a block's point using its ancestor lineage as context.
    pub async fn reduce_lineage(&self, lineage: &Lineage) -> Result<String, LlmError> {
        if lineage.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::reduce_from_lineage(lineage);
        self.request_completion("reduce", prompt, 0.2, 200).await
    }

    /// Expand one target point into rewrite and concise child point candidates.
    pub async fn expand_lineage(&self, lineage: &Lineage) -> Result<ExpandResult, LlmError> {
        if lineage.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::expand_from_lineage(lineage);
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
    fn reduce_from_lineage(lineage: &Lineage) -> Self {
        let mut context_lines = String::new();
        let total = lineage.items.len();
        for (index, item) in lineage.iter().enumerate() {
            let label = if index + 1 == total { "Target" } else { "Parent" };
            context_lines.push_str(&format!("{label}: {}\n", item.point()));
        }

        Self {
            system: "You reduce a bullet point using its ancestors as context. Output a single concise sentence. No quotes, no extra bullet points."
                .to_string(),
            user: format!("Reduce the target point with context:\n{context_lines}"),
        }
    }

    fn expand_from_lineage(lineage: &Lineage) -> Self {
        let mut context_lines = String::new();
        let total = lineage.items.len();
        for (index, item) in lineage.iter().enumerate() {
            let label = if index + 1 == total { "Target" } else { "Parent" };
            context_lines.push_str(&format!("{label}: {}\n", item.point()));
        }

        Self {
            system: "You expand one target bullet point using its ancestors as context. Return strict JSON only with this shape: {\"rewrite\": string|null, \"children\": string[]}. Keep rewrite concise. Generate 3-6 concise child points. No markdown, no extra keys."
                .to_string(),
            user: format!("Expand the target point with context:\n{context_lines}"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ExpandResponsePayload {
    rewrite: Option<String>,
    #[serde(default)]
    children: Vec<String>,
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
        let prompt = Prompt::reduce_from_lineage(&lineage);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
    }

    #[test]
    fn expand_prompt_labels_target_last() {
        let lineage = Lineage::from_points(vec!["first".into(), "second".into(), "third".into()]);
        let prompt = Prompt::expand_from_lineage(&lineage);
        assert!(prompt.user.contains("Parent: first"));
        assert!(prompt.user.contains("Parent: second"));
        assert!(prompt.user.contains("Target: third"));
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
}
