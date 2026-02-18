use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf, sync::LazyLock};
use thiserror::Error;
use tracing;

static PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
    LazyLock::new(|| ProjectDirs::from("app", "miorin", "bb"));

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
        PROJECT_DIRS.as_ref().map(|project| project.config_dir().join("llm.toml"))
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

pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self { config, http: reqwest::Client::new() }
    }

    pub async fn summarize_lineage(&self, lineage: &Lineage) -> Result<String, LlmError> {
        if lineage.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let url = self.chat_url();
        let prompt = Prompt::from_lineage(lineage);
        tracing::info!(model = %self.config.model, url = %url, "llm summarize request");
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message { role: Role::System, content: prompt.system },
                Message { role: Role::User, content: prompt.user },
            ],
            temperature: 0.2,
            max_completion_tokens: 200,
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

        tracing::info!(chars = content.len(), "llm summarize response");
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

struct Prompt {
    system: String,
    user: String,
}

impl Prompt {
    fn from_lineage(lineage: &Lineage) -> Self {
        let mut context_lines = String::new();
        let total = lineage.items.len();
        for (index, item) in lineage.iter().enumerate() {
            let label = if index + 1 == total { "Target" } else { "Parent" };
            context_lines.push_str(&format!("{label}: {}\n", item.point()));
        }

        Self {
            system: "You summarize a bullet point using its ancestors as context. Output a single concise sentence. No quotes, no extra bullet points."
                .to_string(),
            user: format!("Summarize the target point with context:\n{context_lines}"),
        }
    }
}
