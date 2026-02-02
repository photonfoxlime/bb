use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{env, error::Error, fmt, fs, io, path::PathBuf, sync::LazyLock};

static PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
    LazyLock::new(|| ProjectDirs::from("app", "miorin", "bb"));

#[derive(Clone, Serialize, Deserialize)]
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
    fn from_file() -> Result<Option<Self>, LlmConfigError> {
        let Some(path) = Self::config_path() else {
            return Ok(None);
        };
        match fs::read_to_string(&path) {
            | Ok(contents) => toml::from_str(&contents).map(Some).map_err(|err| {
                LlmConfigError::ConfigFile(format!("failed to parse {}: {err}", path.display()))
            }),
            | Err(err) if err.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|err| {
                        LlmConfigError::ConfigFile(format!(
                            "failed to create config directory {}: {err}",
                            parent.display()
                        ))
                    })?;
                }
                fs::write(&path, Self::default_template()).map_err(|err| {
                    LlmConfigError::ConfigFile(format!(
                        "failed to create config file {}: {err}",
                        path.display()
                    ))
                })?;
                Ok(None)
            }
            | Err(err) => {
                Err(LlmConfigError::ConfigFile(format!("failed to read {}: {err}", path.display())))
            }
        }
    }

    fn from_env_or_file() -> Result<Self, LlmConfigError> {
        let mut base_url = env::var("LLM_BASE_URL").ok();
        let mut api_key = env::var("LLM_API_KEY").ok();
        let mut model = env::var("LLM_MODEL").ok();

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
            return Err(LlmConfigError::InvalidConfig("LLM_BASE_URL must start with https://"));
        }
        if api_key.is_empty() {
            return Err(LlmConfigError::InvalidConfig("LLM_API_KEY is empty"));
        }
        if model.is_empty() {
            return Err(LlmConfigError::InvalidConfig("LLM_MODEL is empty"));
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

pub fn load_config() -> Result<LlmConfig, LlmConfigError> {
    LlmConfig::from_env_or_file()
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmConfigError {
    MissingConfig,
    InvalidConfig(&'static str),
    ConfigFile(String),
}

impl fmt::Display for LlmConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | LlmConfigError::MissingConfig => {
                write!(f, "missing LLM config (env vars or config file)")
            }
            | LlmConfigError::InvalidConfig(message) => write!(f, "invalid LLM config: {message}"),
            | LlmConfigError::ConfigFile(message) => write!(f, "LLM config file error: {message}"),
        }
    }
}

impl Error for LlmConfigError {}

#[derive(Debug)]
pub enum LlmError {
    Config(LlmConfigError),
    Http(reqwest::Error),
    InvalidResponse,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            | LlmError::Config(err) => write!(f, "{err}"),
            | LlmError::Http(err) => write!(f, "request failed: {err}"),
            | LlmError::InvalidResponse => write!(f, "invalid response"),
        }
    }
}

impl Error for LlmError {}

impl From<LlmConfigError> for LlmError {
    fn from(err: LlmConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<reqwest::Error> for LlmError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
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

pub async fn summarize_lineage(config: &LlmConfig, lineage: &[String]) -> Result<String, LlmError> {
    if lineage.is_empty() {
        return Err(LlmError::InvalidResponse);
    }

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let (system, user) = build_prompt(lineage);
    let request = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            Message { role: "system".to_string(), content: system },
            Message { role: "user".to_string(), content: user },
        ],
        temperature: 0.2,
        max_tokens: 200,
    };

    let client = reqwest::Client::new();
    let response: ChatResponse = client
        .post(url)
        .bearer_auth(config.api_key.clone())
        .json(&request)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content.trim().to_string())
        .filter(|content| !content.is_empty())
        .ok_or(LlmError::InvalidResponse)
}

fn build_prompt(lineage: &[String]) -> (String, String) {
    let mut context_lines = String::new();
    for (index, item) in lineage.iter().enumerate() {
        let label = if index + 1 == lineage.len() { "Target" } else { "Parent" };
        context_lines.push_str(&format!("{label}: {item}\n"));
    }

    (
        "You summarize a bullet point using its ancestors as context. Output a single concise sentence. No quotes, no extra bullet points."
            .to_string(),
        format!(
            "Summarize the target point with context:\n{context_lines}"
        ),
    )
}
