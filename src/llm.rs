use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{env, error::Error, fmt, fs, path::PathBuf, sync::LazyLock};

static PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
    LazyLock::new(|| ProjectDirs::from("app", "miorin", "bb"));

#[derive(Clone, Serialize, Deserialize)]
struct LlmConfig {
    base_url: String,
    api_key: String,
    model: String,
}

impl LlmConfig {
    fn from_file() -> Option<Self> {
        let path = Self::config_path()?;
        let contents = fs::read_to_string(path).ok()?;
        toml::from_str(&contents).ok()
    }

    fn from_env_or_file() -> Result<Self, LlmError> {
        let mut base_url = env::var("LLM_BASE_URL").ok();
        let mut api_key = env::var("LLM_API_KEY").ok();
        let mut model = env::var("LLM_MODEL").ok();

        if let Some(file_config) = Self::from_file() {
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
            return Err(LlmError::MissingConfig);
        };
        let Some(api_key) = api_key else {
            return Err(LlmError::MissingConfig);
        };
        let Some(model) = model else {
            return Err(LlmError::MissingConfig);
        };

        let base_url = base_url.trim().to_string();
        let api_key = api_key.trim().to_string();
        let model = model.trim().to_string();

        if !base_url.starts_with("https://") {
            return Err(LlmError::InvalidConfig(
                "LLM_BASE_URL must start with https://",
            ));
        }
        if api_key.is_empty() {
            return Err(LlmError::InvalidConfig("LLM_API_KEY is empty"));
        }
        if model.is_empty() {
            return Err(LlmError::InvalidConfig("LLM_MODEL is empty"));
        }

        Ok(Self {
            base_url,
            api_key,
            model,
        })
    }

    fn config_path() -> Option<PathBuf> {
        PROJECT_DIRS
            .as_ref()
            .map(|project| project.config_dir().join("llm.toml"))
    }
}

#[derive(Debug)]
pub enum LlmError {
    MissingConfig,
    InvalidConfig(&'static str),
    Http(reqwest::Error),
    InvalidResponse,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::MissingConfig => write!(f, "missing LLM config (env vars or config file)"),
            LlmError::InvalidConfig(message) => write!(f, "invalid LLM config: {message}"),
            LlmError::Http(err) => write!(f, "request failed: {err}"),
            LlmError::InvalidResponse => write!(f, "invalid response"),
        }
    }
}

impl Error for LlmError {}

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

pub async fn summarize_lineage(lineage: &[String]) -> Result<String, LlmError> {
    if lineage.is_empty() {
        return Err(LlmError::InvalidResponse);
    }

    let config = LlmConfig::from_env_or_file()?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let (system, user) = build_prompt(lineage);
    let request = ChatRequest {
        model: config.model,
        messages: vec![
            Message {
                role: "system".to_string(),
                content: system,
            },
            Message {
                role: "user".to_string(),
                content: user,
            },
        ],
        temperature: 0.2,
        max_tokens: 200,
    };

    let client = reqwest::Client::new();
    let response: ChatResponse = client
        .post(url)
        .bearer_auth(config.api_key)
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
        let label = if index + 1 == lineage.len() {
            "Target"
        } else {
            "Parent"
        };
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
