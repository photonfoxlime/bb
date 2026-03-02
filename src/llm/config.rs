//! LLM configuration: providers, presets, and validated config.
//! The providers speak the OpenAI-compatible chat completions API.
//!
//! # Provider model
//!
//! Providers come in two flavours:
//!
//! - Preset providers ([`PresetProvider`]) are well-known OpenAI-compatible
//!   services whose base URLs are fixed at compile time. Users only supply an
//!   API key.
//! - Custom providers ([`CustomProvider`]) are fully user-defined with
//!   editable name, base URL, and API key.
//!
//! Note: model selection is **not** part of the provider config. Each LLM task
//! (reduce, expand, inquire) independently selects its own provider + model via
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

/// Validated LLM endpoint configuration.
///
/// Invariants (enforced by [`LlmConfig::from_raw`]):
/// - `base_url` starts with `https://`
/// - `api_key` and `model` are non-empty after trimming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) model: String,
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
    /// Validate and construct a config from raw string fields.
    ///
    /// Trims whitespace from all fields and enforces invariants.
    pub fn from_raw(
        base_url: String, api_key: String, model: String,
    ) -> Result<Self, LlmConfigError> {
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
}

/// Known OpenAI-compatible LLM service with a fixed API endpoint.
///
/// Each variant carries its base URL and a suggested default model.
/// The `serde` representation is a lowercase string so that TOML keys
/// like `[presets.openai]` deserialise naturally.
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
}

/// All preset provider variants in display order.
const ALL_PRESETS: &[PresetProvider] = &[
    PresetProvider::OpenAI,
    PresetProvider::OpenRouter,
    PresetProvider::DeepSeek,
    PresetProvider::Gemini,
    PresetProvider::Groq,
];

/// Default provider name used for per-task config defaults.
pub const DEFAULT_PROVIDER: &str = "openai";

impl PresetProvider {
    /// Compile-time base URL for this provider's chat completions endpoint.
    pub fn base_url(self) -> &'static str {
        match self {
            | Self::OpenAI => "https://api.openai.com/v1",
            | Self::OpenRouter => "https://openrouter.ai/api/v1",
            | Self::DeepSeek => "https://api.deepseek.com",
            | Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            | Self::Groq => "https://api.groq.com/openai/v1",
        }
    }

    /// Suggested model name for new configurations. Empty when the service
    /// offers too many models to pick a sensible default.
    pub fn default_model(self) -> &'static str {
        match self {
            | Self::OpenAI => "gpt-4o",
            | Self::DeepSeek => "deepseek-chat",
            | Self::Gemini => "gemini-2.0-flash",
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
    /// OpenAI-compatible chat completions endpoint URL.
    pub base_url: String,
    /// API key for authentication.
    pub api_key: String,
}

impl Default for CustomProvider {
    fn default() -> Self {
        Self { base_url: "https://api.example.com/v1".to_string(), api_key: String::new() }
    }
}

/// Named collection of preset and custom LLM providers.
///
/// Invariants:
/// - Every [`PresetProvider`] variant has an entry in `presets`.
///
/// Note: there is no global "active" provider. Each LLM task (reduce, expand,
/// inquire) independently selects a provider + model via
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
        let (mut base_url, mut api_key) = self.raw_fields(provider_name).unwrap_or_else(|| {
            // Provider name doesn't match anything; use empty defaults.
            (String::new(), String::new())
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

        LlmConfig::from_raw(base_url, api_key, model)
    }

    /// Ordered list of all provider names (presets first, then custom).
    pub fn provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = ALL_PRESETS.iter().map(|p| p.name().to_string()).collect();
        names.extend(self.custom.keys().cloned());
        names
    }

    /// Raw `(base_url, api_key)` tuple for a provider by name.
    ///
    /// For presets the base URL comes from [`PresetProvider::base_url`].
    pub fn raw_fields(&self, name: &str) -> Option<(String, String)> {
        if let Some(preset) = PresetProvider::from_name(name) {
            let config = self.presets.get(&preset).cloned().unwrap_or_default();
            return Some((preset.base_url().to_string(), config.api_key));
        }
        self.custom.get(name).map(|c| (c.base_url.clone(), c.api_key.clone()))
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
