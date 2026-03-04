//! LLM configuration: providers, presets, and validated config.
//!
//! # Provider model
//!
//! Providers come in two flavours:
//!
//! - Preset providers ([`PresetProvider`]) are well-known LLM services whose
//!   base URLs are fixed at compile time. Users only supply an API key.
//! - Custom providers ([`CustomProvider`]) are fully user-defined with
//!   editable name, base URL, API key, and API style.
//!
//! Each provider has an [`ApiStyle`] that determines the wire format:
//! - [`ApiStyle::OpenAi`] for the OpenAI-compatible chat completions API.
//! - [`ApiStyle::Anthropic`] for the Anthropic Messages API.
//!
//! Note: model selection is **not** part of the provider config. Each LLM task
//! (amplify, distill, atomize, probe) independently selects its own provider + model via
//! [`crate::app::config::TaskSettings`].
//!
//! [`LlmProviders`] stores both sets. The on-disk format is a single TOML file
//! at [`crate::paths::AppPaths::llm_config()`]:
//!
//! ```toml
//! [presets.openai]
//! api_key = "sk-..."
//!
//! [custom.my-local]
//! base_url = "https://my-proxy.example.com/v1"
//! api_key  = "sk-..."
//! ```
//!
//! Environment variables (`LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`)
//! override fields during [`LlmProviders::resolve`].

use super::error::{ConfigFileError, InvalidConfigReason, LlmConfigError};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, fmt, fs, io, path::PathBuf};

/// Wire format used to communicate with an LLM endpoint.
///
/// Determines how requests are serialized, how authentication headers are set,
/// and how responses (including streaming SSE) are parsed.
///
/// Defaults to [`ApiStyle::OpenAi`] so that existing configs without this
/// field deserialize without breaking (`#[serde(default)]`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiStyle {
    /// OpenAI-compatible chat completions API (`/chat/completions`).
    /// Uses `Authorization: Bearer <key>` for auth.
    #[default]
    OpenAi,
    /// Anthropic Messages API (`/messages`).
    /// Uses `x-api-key` header and `anthropic-version` header for auth.
    Anthropic,
}

impl ApiStyle {
    /// All available API styles in display order.
    pub const ALL: &[ApiStyle] = &[ApiStyle::OpenAi, ApiStyle::Anthropic];

    /// Human-readable label for UI display.
    pub fn label(self) -> &'static str {
        match self {
            | Self::OpenAi => "OpenAI",
            | Self::Anthropic => "Anthropic",
        }
    }
}

impl fmt::Display for ApiStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Identifies one of the four LLM task categories.
///
/// Used for per-task settings (provider, model, prompts), prompt construction,
/// and UI discriminants.
///
/// # Action semantics
///
/// - **Amplify**: Add detail, examples, and context while keeping the original
///   intent. Produces an optional rewrite and child point suggestions.
/// - **Distill**: Summarize into a clearer, shorter version, keeping essential
///   information. May mark existing children as redundant for removal.
/// - **Atomize**: Break the text into a list of distinct information points
///   without dropping details. Produces an optional parent rewrite and point list.
/// - **Probe**: Ask targeted questions to clarify meaning, fill gaps, or
///   challenge assumptions. Requires user instruction; returns a free-form response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskKind {
    /// Add detail, examples, context; produces rewrite + child suggestions.
    Amplify,
    /// Summarize into a shorter version; may mark children as redundant.
    Distill,
    /// Break text into distinct information points without dropping details.
    Atomize,
    /// Ask targeted questions; requires instruction, returns free-form response.
    Probe,
}

/// Validated LLM endpoint configuration.
///
/// Invariants (enforced by [`LlmConfig::from_raw`]):
/// - `base_url` starts with `https://` or `http://localhost` or `http://127.0.0.1`
/// - `api_key` and `model` are non-empty after trimming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) model: String,
    /// Wire format for this endpoint. Determines request serialization,
    /// auth headers, and response parsing.
    #[serde(default)]
    pub(crate) api_style: ApiStyle,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.example.com/v1".to_string(),
            api_key: String::new(),
            model: String::new(),
            api_style: ApiStyle::default(),
        }
    }
}

impl LlmConfig {
    /// Validate and construct a config from raw string fields.
    ///
    /// Trims whitespace from all fields and enforces invariants.
    pub fn from_raw(
        base_url: String, api_key: String, model: String, api_style: ApiStyle,
    ) -> Result<Self, LlmConfigError> {
        let base_url = base_url.trim().to_string();
        let api_key = api_key.trim().to_string();
        let model = model.trim().to_string();

        let base_url_valid = base_url.starts_with("https://")
            || base_url.starts_with("http://localhost")
            || base_url.starts_with("http://127.0.0.1");
        if !base_url_valid {
            return Err(LlmConfigError::InvalidConfig(InvalidConfigReason::BaseUrlNotHttps));
        }
        if api_key.is_empty() {
            return Err(LlmConfigError::InvalidConfig(InvalidConfigReason::ApiKeyEmpty));
        }
        if model.is_empty() {
            return Err(LlmConfigError::InvalidConfig(InvalidConfigReason::ModelEmpty));
        }

        Ok(Self { base_url, api_key, model, api_style })
    }

    fn config_path() -> Option<PathBuf> {
        AppPaths::llm_config()
    }
}

/// Known LLM service with a fixed API endpoint.
///
/// Each variant carries its base URL, a suggested default model, and
/// its [`ApiStyle`]. The `serde` representation is a lowercase string so
/// that TOML keys like `[presets.openai]` deserialise naturally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetProvider {
    /// <https://api.openai.com/v1>
    OpenAI,
    /// <https://openrouter.ai/api/v1>
    OpenRouter,
    /// <https://api.deepseek.com>
    DeepSeek,
    /// <https://generativelanguage.googleapis.com/v1beta/openai>
    Gemini,
    /// <https://api.groq.com/openai/v1>
    Groq,
    /// <https://api.anthropic.com/v1>
    Anthropic,
    /// <https://api.moonshot.ai/v1> — Kimi (Moonshot AI)
    Kimi,
    /// <https://api.minimax.io/v1> — OpenAI-compatible endpoint
    Minimax,
    /// <http://localhost:11434/v1> — local models via Ollama (OpenAI-compatible)
    Ollama,
}

/// All preset provider variants in display order.
const ALL_PRESETS: &[PresetProvider] = &[
    PresetProvider::OpenAI,
    PresetProvider::OpenRouter,
    PresetProvider::DeepSeek,
    PresetProvider::Gemini,
    PresetProvider::Groq,
    PresetProvider::Anthropic,
    PresetProvider::Kimi,
    PresetProvider::Minimax,
    PresetProvider::Ollama,
];

/// Default provider name used for per-task config defaults.
pub const DEFAULT_PROVIDER: &str = "openai";

impl PresetProvider {
    /// Compile-time base URL for this provider's API endpoint.
    pub fn base_url(self) -> &'static str {
        match self {
            | Self::OpenAI => "https://api.openai.com/v1",
            | Self::OpenRouter => "https://openrouter.ai/api/v1",
            | Self::DeepSeek => "https://api.deepseek.com",
            | Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            | Self::Groq => "https://api.groq.com/openai/v1",
            | Self::Anthropic => "https://api.anthropic.com/v1",
            | Self::Kimi => "https://api.moonshot.ai/v1",
            | Self::Minimax => "https://api.minimax.io/v1",
            | Self::Ollama => "http://localhost:11434/v1",
        }
    }

    /// Suggested model name for new configurations. Empty when the service
    /// offers too many models to pick a sensible default.
    pub fn default_model(self) -> &'static str {
        match self {
            | Self::OpenAI => "gpt-5.2",
            | Self::DeepSeek => "deepseek-chat",
            | Self::Gemini => "gemini-2.5-flash",
            | Self::Anthropic => "claude-sonnet-4-6",
            | Self::Kimi => "kimi-k2.5",
            | Self::Minimax => "MiniMax-M2.5",
            | Self::Ollama => "llama3.2",
            | Self::OpenRouter | Self::Groq => "",
        }
    }

    /// Lowercase name used as the provider key in config files and UI.
    pub fn name(self) -> &'static str {
        match self {
            | Self::OpenAI => "openai",
            | Self::OpenRouter => "openrouter",
            | Self::DeepSeek => "deepseek",
            | Self::Gemini => "gemini",
            | Self::Groq => "groq",
            | Self::Anthropic => "anthropic",
            | Self::Kimi => "kimi",
            | Self::Minimax => "minimax",
            | Self::Ollama => "ollama",
        }
    }

    /// Wire format used by this preset provider.
    pub fn api_style(self) -> ApiStyle {
        match self {
            | Self::Anthropic => ApiStyle::Anthropic,
            | Self::OpenAI
            | Self::OpenRouter
            | Self::DeepSeek
            | Self::Gemini
            | Self::Groq
            | Self::Kimi
            | Self::Minimax
            | Self::Ollama => ApiStyle::OpenAi,
        }
    }

    /// Look up a preset by its lowercase name.
    pub fn from_name(name: &str) -> Option<Self> {
        ALL_PRESETS.iter().copied().find(|p| p.name() == name)
    }
}

impl fmt::Display for PresetProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// User configuration for a preset provider.
///
/// The base URL is fixed by the [`PresetProvider`] variant; only the API key
/// is stored. An empty `api_key` means the user has not configured this
/// preset yet (validation happens at [`LlmProviders::resolve`]).
///
/// Note: model selection lives in per-task [`crate::app::config::TaskConfig`],
/// not here. Legacy `model` fields in TOML are silently ignored on load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PresetConfig {
    /// API key for authentication. May be empty if not yet configured.
    #[serde(default)]
    pub api_key: String,
}

/// Fully user-defined LLM provider with all fields editable.
///
/// Note: model selection lives in per-task [`crate::app::config::TaskConfig`],
/// not here. Legacy `model` fields in TOML are silently ignored on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvider {
    /// API endpoint URL.
    pub base_url: String,
    /// API key for authentication.
    pub api_key: String,
    /// Wire format for this endpoint. Defaults to [`ApiStyle::OpenAi`] when
    /// absent in existing TOML files (backward compatible).
    #[serde(default)]
    pub api_style: ApiStyle,
}

impl Default for CustomProvider {
    fn default() -> Self {
        Self {
            base_url: "https://api.example.com/v1".to_string(),
            api_key: String::new(),
            api_style: ApiStyle::default(),
        }
    }
}

/// Named collection of preset and custom LLM providers.
///
/// Invariants:
/// - Every [`PresetProvider`] variant has an entry in `presets`.
///
/// Note: there is no global "active" provider. Each LLM task (amplify, distill,
/// atomize, probe) independently selects a provider + model via
/// [`crate::app::config::TaskConfig`]. Legacy `active` fields in TOML are
/// silently ignored on load.
///
/// The TOML representation uses `[presets.<name>]` and `[custom.<name>]` tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviders {
    /// Preset provider configurations keyed by [`PresetProvider`] variant.
    presets: BTreeMap<PresetProvider, PresetConfig>,
    /// User-defined provider configurations keyed by display name.
    #[serde(default)]
    custom: BTreeMap<String, CustomProvider>,
}

impl Default for LlmProviders {
    fn default() -> Self {
        let presets = ALL_PRESETS.iter().map(|&p| (p, PresetConfig::default())).collect();
        Self { presets, custom: BTreeMap::new() }
    }
}

impl LlmProviders {
    /// Load providers from the TOML config file, falling back to defaults
    /// if no file exists.
    ///
    /// Environment variable overrides are applied lazily during
    /// [`resolve`](Self::resolve), not at load time.
    pub fn load() -> Result<Self, LlmConfigError> {
        Self::from_file()
    }

    /// Resolve a provider by name and model into a validated [`LlmConfig`].
    ///
    /// Applies `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL` env var overrides
    /// on top of the stored fields before validation.
    pub fn resolve(&self, provider_name: &str, model: &str) -> Result<LlmConfig, LlmConfigError> {
        let (mut base_url, mut api_key, api_style) =
            self.raw_fields(provider_name).unwrap_or_else(|| {
                // Provider name doesn't match anything; use empty defaults.
                (String::new(), String::new(), ApiStyle::default())
            });
        let mut model = model.to_string();

        // Apply env-var overrides.
        fn env_non_empty(var: &str) -> Option<String> {
            env::var(var).ok().filter(|v| !v.is_empty())
        }
        if let Some(url) = env_non_empty("LLM_BASE_URL") {
            base_url = url;
        }
        if let Some(key) = env_non_empty("LLM_API_KEY") {
            api_key = key;
        }
        if let Some(m) = env_non_empty("LLM_MODEL") {
            model = m;
        }

        // Ollama ignores auth; use placeholder when empty so validation passes.
        let api_key = if provider_name == "ollama" && api_key.is_empty() {
            "ollama".to_string()
        } else {
            api_key
        };

        LlmConfig::from_raw(base_url, api_key, model, api_style)
    }

    /// Ordered list of all provider names (presets first, then custom).
    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = ALL_PRESETS.iter().map(|p| p.name().to_string()).collect();
        names.extend(self.custom.keys().cloned());
        names
    }

    /// Raw `(base_url, api_key, api_style)` tuple for a provider by name.
    ///
    /// For presets the base URL and API style come from [`PresetProvider`].
    pub fn raw_fields(&self, name: &str) -> Option<(String, String, ApiStyle)> {
        if let Some(preset) = PresetProvider::from_name(name) {
            let config = self.presets.get(&preset).cloned().unwrap_or_default();
            return Some((preset.base_url().to_string(), config.api_key, preset.api_style()));
        }
        self.custom.get(name).map(|c| (c.base_url.clone(), c.api_key.clone(), c.api_style))
    }

    /// Whether a provider with the given name exists (preset or custom).
    pub fn provider_exists(&self, name: &str) -> bool {
        PresetProvider::from_name(name).is_some() || self.custom.contains_key(name)
    }

    /// Whether the given provider name refers to a preset.
    pub fn is_preset(&self, name: &str) -> bool {
        PresetProvider::from_name(name).is_some()
    }

    /// Update the stored configuration for a preset provider.
    pub fn update_preset(&mut self, provider: PresetProvider, config: PresetConfig) {
        self.presets.insert(provider, config);
    }

    /// Insert or update a custom provider configuration.
    ///
    /// The name must not collide with a preset provider name.
    pub fn upsert_custom(
        &mut self, name: String, provider: CustomProvider,
    ) -> Result<(), LlmConfigError> {
        if PresetProvider::from_name(&name).is_some() {
            return Err(LlmConfigError::NameCollision(name));
        }
        self.custom.insert(name, provider);
        Ok(())
    }

    /// Remove a provider by name.
    ///
    /// Preset providers cannot be removed.
    pub fn remove_provider(&mut self, name: &str) -> Result<(), LlmConfigError> {
        if PresetProvider::from_name(name).is_some() {
            return Err(LlmConfigError::CannotRemovePreset);
        }
        if self.custom.remove(name).is_none() {
            return Err(LlmConfigError::ProviderNotFound(name.to_string()));
        }
        Ok(())
    }

    /// Persist the current providers to the TOML config file.
    pub fn save_to_file(&self) -> Result<(), LlmConfigError> {
        let Some(path) = LlmConfig::config_path() else {
            return Err(LlmConfigError::MissingConfig);
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                LlmConfigError::from(ConfigFileError::create_dir(parent.to_path_buf(), err))
            })?;
        }
        let body = toml::to_string_pretty(self).expect("LlmProviders is always serializable");
        fs::write(&path, body)
            .map_err(|err| LlmConfigError::from(ConfigFileError::write(path, err)))
    }

    /// Load from the TOML file, writing defaults when no file exists.
    fn from_file() -> Result<Self, LlmConfigError> {
        let Some(path) = LlmConfig::config_path() else {
            return Err(LlmConfigError::MissingConfig);
        };
        match fs::read_to_string(&path) {
            | Ok(contents) => {
                let providers: Self = toml::from_str(&contents).map_err(|err| {
                    LlmConfigError::from(ConfigFileError::parse(path.clone(), err))
                })?;
                Ok(providers.ensure_all_presets())
            }
            | Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let defaults = Self::default();
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|err| {
                        LlmConfigError::from(ConfigFileError::create_dir(parent.to_path_buf(), err))
                    })?;
                }
                let body = toml::to_string_pretty(&defaults)
                    .expect("default LlmProviders is always serializable");
                fs::write(&path, body).map_err(|err| {
                    LlmConfigError::from(ConfigFileError::write(path.clone(), err))
                })?;
                Ok(defaults)
            }
            | Err(err) => Err(LlmConfigError::from(ConfigFileError::read(path, err))),
        }
    }

    /// Ensure every preset variant has an entry, filling missing ones with defaults.
    fn ensure_all_presets(mut self) -> Self {
        for &preset in ALL_PRESETS {
            self.presets.entry(preset).or_default();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl LlmProviders {
        /// Create a provider set with a single valid preset config for testing.
        ///
        /// The openai preset has a valid API key so that
        /// `resolve("openai", "test-model")` succeeds.
        pub fn test_valid() -> Self {
            let mut providers = Self::default();
            providers.update_preset(
                PresetProvider::OpenAI,
                PresetConfig { api_key: "test-key".to_string() },
            );
            providers
        }
    }

    #[test]
    fn ollama_resolves_with_empty_api_key() {
        let providers = LlmProviders::default();
        let config = providers.resolve("ollama", "llama3.2").expect("ollama should resolve");
        assert_eq!(config.base_url, "http://localhost:11434/v1");
        assert_eq!(config.model, "llama3.2");
        assert!(!config.api_key.is_empty());
    }
}
