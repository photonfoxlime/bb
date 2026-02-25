//! LLM integration: configuration, prompt construction, and API client.
//!
//! The client speaks the OpenAI-compatible chat completions API.
//!
//! # Provider model
//!
//! Providers come in two flavours:
//!
//! - **Preset providers** ([`PresetProvider`]) are well-known OpenAI-compatible
//!   services whose base URLs are fixed at compile time. Users only supply an
//!   API key (and optionally override the default model).
//! - **Custom providers** ([`CustomProvider`]) are fully user-defined with
//!   editable name, base URL, API key, and model.
//!
//! [`LlmProviders`] stores both sets and selects one as `active`. The on-disk
//! format is a single TOML file at [`AppPaths::llm_config()`]:
//!
//! ```toml
//! active = "openai"
//!
//! [presets.openai]
//! api_key = "sk-..."
//! model   = "gpt-4o"
//!
//! [custom.my-local]
//! base_url = "https://my-proxy.example.com/v1"
//! api_key  = "sk-..."
//! model    = "llama-3"
//! ```
//!
//! Environment variables (`LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`)
//! override fields on the **active** provider during [`LlmProviders::resolve_active`].

use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, env, fmt, fs, io, path::PathBuf};
use thiserror::Error;

/// Validated LLM endpoint configuration.
///
/// Invariants (enforced by [`LlmConfig::from_raw`]):
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

/// Default active provider name (must match a [`PresetProvider`] variant).
const DEFAULT_ACTIVE_PROVIDER: &str = "openai";

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
/// and model override are stored. An empty `api_key` means the user has not
/// configured this preset yet (validation happens at [`LlmProviders::resolve_active`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PresetConfig {
    /// API key for authentication. May be empty if not yet configured.
    #[serde(default)]
    pub api_key: String,
    /// Model override. If empty, [`PresetProvider::default_model`] is used.
    #[serde(default)]
    pub model: String,
}

/// Fully user-defined LLM provider with all fields editable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvider {
    /// OpenAI-compatible chat completions endpoint URL.
    pub base_url: String,
    /// API key for authentication.
    pub api_key: String,
    /// Model identifier.
    pub model: String,
}

impl Default for CustomProvider {
    fn default() -> Self {
        Self {
            base_url: "https://api.example.com/v1".to_string(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

/// Named collection of preset and custom LLM providers with one active selection.
///
/// Invariants:
/// - Every [`PresetProvider`] variant has an entry in `presets`.
/// - `active` always refers to either a preset name or a key in `custom`.
///
/// The TOML representation uses `active = "name"` plus `[presets.<name>]`
/// and `[custom.<name>]` tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviders {
    /// Name of the currently active provider (preset or custom).
    active: String,
    /// Preset provider configurations keyed by [`PresetProvider`] variant.
    presets: BTreeMap<PresetProvider, PresetConfig>,
    /// User-defined provider configurations keyed by display name.
    #[serde(default)]
    custom: BTreeMap<String, CustomProvider>,
}

impl Default for LlmProviders {
    fn default() -> Self {
        let presets = ALL_PRESETS.iter().map(|&p| (p, PresetConfig::default())).collect();
        Self { active: DEFAULT_ACTIVE_PROVIDER.to_string(), presets, custom: BTreeMap::new() }
    }
}

impl LlmProviders {
    /// Load providers from the TOML config file, falling back to defaults
    /// if no file exists.
    ///
    /// Environment variable overrides are applied lazily during
    /// [`resolve_active`](Self::resolve_active), not at load time.
    pub fn load() -> Result<Self, LlmConfigError> {
        Self::from_file()
    }

    /// Name of the currently active provider.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// Resolve the active provider into a validated [`LlmConfig`].
    ///
    /// Applies `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL` env var overrides
    /// on top of the stored fields before validation.
    pub fn resolve_active(&self) -> Result<LlmConfig, LlmConfigError> {
        let (mut base_url, mut api_key, mut model) =
            self.raw_fields(&self.active).unwrap_or_else(|| {
                // Active name doesn't match anything; use empty defaults.
                (String::new(), String::new(), String::new())
            });

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

    /// Raw `(base_url, api_key, model)` tuple for a provider by name.
    ///
    /// For presets the base URL comes from [`PresetProvider::base_url`]
    /// and the model falls back to [`PresetProvider::default_model`] when empty.
    pub fn raw_fields(&self, name: &str) -> Option<(String, String, String)> {
        if let Some(preset) = PresetProvider::from_name(name) {
            let config = self.presets.get(&preset).cloned().unwrap_or_default();
            let model = if config.model.is_empty() {
                preset.default_model().to_string()
            } else {
                config.model
            };
            return Some((preset.base_url().to_string(), config.api_key, model));
        }
        self.custom.get(name).map(|c| (c.base_url.clone(), c.api_key.clone(), c.model.clone()))
    }

    /// Whether a provider with the given name exists (preset or custom).
    pub fn provider_exists(&self, name: &str) -> bool {
        PresetProvider::from_name(name).is_some() || self.custom.contains_key(name)
    }

    /// Whether the given provider name refers to a preset.
    pub fn is_preset(&self, name: &str) -> bool {
        PresetProvider::from_name(name).is_some()
    }

    /// Set the active provider, returning an error if the name does not exist.
    pub fn set_active(&mut self, name: &str) -> Result<(), LlmConfigError> {
        if !self.provider_exists(name) {
            return Err(LlmConfigError::ProviderNotFound(name.to_string()));
        }
        self.active = name.to_string();
        Ok(())
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
    /// Preset providers cannot be removed. The currently active provider
    /// cannot be removed (switch active first).
    pub fn remove_provider(&mut self, name: &str) -> Result<(), LlmConfigError> {
        if PresetProvider::from_name(name).is_some() {
            return Err(LlmConfigError::CannotRemovePreset);
        }
        if self.active == name {
            return Err(LlmConfigError::CannotRemoveActive);
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
    /// The active provider (openai) has a valid API key and model so that
    /// `resolve_active()` succeeds.
    pub fn test_valid() -> Self {
        let mut providers = Self::default();
        providers.update_preset(
            PresetProvider::OpenAI,
            PresetConfig { api_key: "test-key".to_string(), model: "test-model".to_string() },
        );
        providers
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
    #[error("provider not found: {0}")]
    ProviderNotFound(String),
    #[error("cannot remove the active provider")]
    CannotRemoveActive,
    #[error("cannot remove a preset provider")]
    CannotRemovePreset,
    #[error("name collides with a preset provider: {0}")]
    NameCollision(String),
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
/// The target block point is represented by the final lineage item.
/// Therefore one `BlockContext` captures the full readable context envelope:
/// target point, parent chain, direct children, and friend blocks.
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
    pub async fn inquire(
        &self, context: &BlockContext, instruction: &str,
    ) -> Result<String, LlmError> {
        if context.is_empty() {
            return Err(LlmError::InvalidRequest);
        }
        if instruction.is_empty() {
            return Err(LlmError::InvalidRequest);
        }

        let prompt = Prompt::inquire_from_context(context, instruction);
        let content = self.request_completion("inquire", prompt, 0.7, 700).await?;

        tracing::info!(chars = content.len(), "llm inquire response");
        Ok(content.trim().to_string())
    }

    async fn request_completion(
        &self, purpose: &'static str, prompt: Prompt, temperature: f32, max_completion_tokens: u32,
    ) -> Result<String, LlmError> {
        let url = self.chat_url();
        tracing::info!(model = %self.config.model, url = %url, purpose, max_completion_tokens, "llm request");
        let (value, body) =
            self.send_completion_request(&url, &prompt, temperature, max_completion_tokens).await?;

        if let Some(content) = extract_completion_content_from_chat_value(&value) {
            tracing::info!(purpose, chars = content.len(), "llm completion response");
            return Ok(content);
        }

        if response_hit_token_limit(&value) {
            let retry_max_tokens = (max_completion_tokens.saturating_mul(2)).min(2_000);
            if retry_max_tokens > max_completion_tokens {
                tracing::warn!(
                    purpose,
                    first_max_completion_tokens = max_completion_tokens,
                    retry_max_completion_tokens = retry_max_tokens,
                    "llm response reached token limit with no extractable text; retrying once"
                );
                let (retry_value, retry_body) = self
                    .send_completion_request(&url, &prompt, temperature, retry_max_tokens)
                    .await?;
                if let Some(content) = extract_completion_content_from_chat_value(&retry_value) {
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

        tracing::error!(
            purpose,
            body_preview = %preview_body(&body),
            finish_reason = ?first_choice_finish_reason(&value),
            completion_tokens = ?completion_tokens(&value),
            "llm response json parsed but no text content could be extracted"
        );
        Err(LlmError::InvalidResponse)
    }

    async fn send_completion_request(
        &self, url: &str, prompt: &Prompt, temperature: f32, max_completion_tokens: u32,
    ) -> Result<(Value, String), LlmError> {
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message { role: Role::System, content: prompt.system.clone() },
                Message { role: Role::User, content: prompt.user.clone() },
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
                system: format!(
                    "{}You reduce a bullet point using its ancestors as context. Return strict JSON only: {{\"reduction\": string}}. The reduction must be a single concise sentence. No markdown, no extra keys.",
                    instruction_prefix
                ),
                user: format!("Reduce the target point with context:\n{lineage_lines}"),
            };
        }

        if context.existing_children.is_empty() {
            return Self {
                system: format!(
                    "{}You reduce a bullet point using its ancestors plus friend blocks as context. Return strict JSON only: {{\"reduction\": string}}. The reduction must be a single concise sentence. Friend blocks are user-selected related context and are not children of the target. Each friend block may include an optional perspective describing how the target views that friend block; use it when helpful. No markdown, no extra keys.",
                    instruction_prefix
                ),
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
            system: format!(
                "{}You reduce a bullet point using its ancestors, existing children, and optional friend blocks as context. Return strict JSON only: {{\"reduction\": string, \"redundant_children\": number[]}}. The reduction must be a single concise sentence that captures the essential meaning. redundant_children: 0-based indices of existing children whose information is fully captured by the reduction and can be safely removed. Friend blocks are additional context only and must never appear in redundant_children. Friend blocks may include optional perspective text that can refine interpretation. Only mark a child redundant when its content is genuinely subsumed. No markdown, no extra keys.",
                instruction_prefix
            ),
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
                system: format!(
                    "{}You expand one target bullet point using its ancestors as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite. No markdown, no extra keys.",
                    instruction_prefix
                ),
                user: format!("Expand the target point with context:\n{lineage_lines}"),
            };
        }

        if context.existing_children.is_empty() {
            return Self {
                system: format!(
                    "{}You expand one target bullet point using its ancestors plus friend blocks as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, and should not restate the rewrite. Friend blocks are user-selected related context and are not children of the target. Friend blocks may include an optional perspective describing how the target views that friend block; use it when relevant. No markdown, no extra keys.",
                    instruction_prefix
                ),
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
            system: format!(
                "{}You expand one target bullet point using its ancestors, existing children, and optional friend blocks as context. Return strict JSON only with this shape: {{\"rewrite\": string|null, \"children\": string[]}}. Keep rewrite to one concise sentence. Generate 3-6 concise NEW child points. Children must be mutually non-overlapping, each focused on a distinct subtopic, should not restate the rewrite, and MUST NOT overlap with the existing children listed below. Friend blocks are additional context only and are not children. Friend blocks may include optional perspective text that can refine interpretation. No markdown, no extra keys.",
                instruction_prefix
            ),
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
        let json = r#"{"choices":[{"text":"  fallback text  "}]}"#;
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

    // ── LlmProviders preset/custom logic ─────────────────────────────

    #[test]
    fn default_has_all_presets() {
        let providers = LlmProviders::default();
        for preset in ALL_PRESETS {
            assert!(providers.provider_exists(preset.name()), "missing preset: {}", preset.name());
        }
    }

    #[test]
    fn default_active_is_openai() {
        let providers = LlmProviders::default();
        assert_eq!(providers.active(), "openai");
    }

    #[test]
    fn provider_names_presets_before_custom() {
        let mut providers = LlmProviders::default();
        providers.upsert_custom("my-local".to_string(), CustomProvider::default()).unwrap();
        let names = providers.provider_names();
        let preset_count = ALL_PRESETS.len();
        // All preset names come first.
        for (i, preset) in ALL_PRESETS.iter().enumerate() {
            assert_eq!(names[i], preset.name());
        }
        // Custom names follow.
        assert_eq!(names[preset_count], "my-local");
    }

    #[test]
    fn is_preset_true_for_known() {
        let providers = LlmProviders::default();
        assert!(providers.is_preset("openai"));
        assert!(providers.is_preset("groq"));
    }

    #[test]
    fn is_preset_false_for_custom() {
        let mut providers = LlmProviders::default();
        providers.upsert_custom("my-local".to_string(), CustomProvider::default()).unwrap();
        assert!(!providers.is_preset("my-local"));
    }

    #[test]
    fn raw_fields_preset_uses_fixed_base_url() {
        let providers = LlmProviders::default();
        let (base_url, _, _) = providers.raw_fields("openai").unwrap();
        assert_eq!(base_url, PresetProvider::OpenAI.base_url());
    }

    #[test]
    fn raw_fields_preset_falls_back_to_default_model() {
        let providers = LlmProviders::default();
        let (_, _, model) = providers.raw_fields("openai").unwrap();
        assert_eq!(model, "gpt-4o");
    }

    #[test]
    fn raw_fields_preset_uses_override_model() {
        let mut providers = LlmProviders::default();
        providers.update_preset(
            PresetProvider::OpenAI,
            PresetConfig { api_key: String::new(), model: "gpt-4-turbo".to_string() },
        );
        let (_, _, model) = providers.raw_fields("openai").unwrap();
        assert_eq!(model, "gpt-4-turbo");
    }

    #[test]
    fn raw_fields_custom_returns_stored_values() {
        let mut providers = LlmProviders::default();
        let custom = CustomProvider {
            base_url: "https://my.api.com/v1".to_string(),
            api_key: "sk-custom".to_string(),
            model: "llama-3".to_string(),
        };
        providers.upsert_custom("my-local".to_string(), custom).unwrap();
        let (base_url, api_key, model) = providers.raw_fields("my-local").unwrap();
        assert_eq!(base_url, "https://my.api.com/v1");
        assert_eq!(api_key, "sk-custom");
        assert_eq!(model, "llama-3");
    }

    #[test]
    fn raw_fields_unknown_returns_none() {
        let providers = LlmProviders::default();
        assert!(providers.raw_fields("nonexistent").is_none());
    }

    #[test]
    fn upsert_custom_succeeds() {
        let mut providers = LlmProviders::default();
        let result = providers.upsert_custom("my-local".to_string(), CustomProvider::default());
        assert!(result.is_ok());
        assert!(providers.provider_exists("my-local"));
    }

    #[test]
    fn upsert_custom_name_collision_rejected() {
        let mut providers = LlmProviders::default();
        let result = providers.upsert_custom("openai".to_string(), CustomProvider::default());
        assert_eq!(result, Err(LlmConfigError::NameCollision("openai".to_string())));
    }

    #[test]
    fn remove_preset_rejected() {
        let mut providers = LlmProviders::default();
        let result = providers.remove_provider("openai");
        assert_eq!(result, Err(LlmConfigError::CannotRemovePreset));
    }

    #[test]
    fn remove_active_rejected() {
        let mut providers = LlmProviders::default();
        providers.upsert_custom("my-local".to_string(), CustomProvider::default()).unwrap();
        providers.set_active("my-local").unwrap();
        let result = providers.remove_provider("my-local");
        assert_eq!(result, Err(LlmConfigError::CannotRemoveActive));
    }

    #[test]
    fn remove_custom_succeeds() {
        let mut providers = LlmProviders::default();
        providers.upsert_custom("my-local".to_string(), CustomProvider::default()).unwrap();
        let result = providers.remove_provider("my-local");
        assert!(result.is_ok());
        assert!(!providers.provider_exists("my-local"));
    }

    #[test]
    fn remove_unknown_rejected() {
        let mut providers = LlmProviders::default();
        let result = providers.remove_provider("nonexistent");
        assert_eq!(result, Err(LlmConfigError::ProviderNotFound("nonexistent".to_string())));
    }

    #[test]
    fn set_active_to_preset_succeeds() {
        let mut providers = LlmProviders::default();
        assert!(providers.set_active("deepseek").is_ok());
        assert_eq!(providers.active(), "deepseek");
    }

    #[test]
    fn set_active_to_custom_succeeds() {
        let mut providers = LlmProviders::default();
        providers.upsert_custom("my-local".to_string(), CustomProvider::default()).unwrap();
        assert!(providers.set_active("my-local").is_ok());
        assert_eq!(providers.active(), "my-local");
    }

    #[test]
    fn set_active_to_unknown_rejected() {
        let mut providers = LlmProviders::default();
        let result = providers.set_active("nonexistent");
        assert_eq!(result, Err(LlmConfigError::ProviderNotFound("nonexistent".to_string())));
    }

    #[test]
    fn update_preset_overwrites() {
        let mut providers = LlmProviders::default();
        let config =
            PresetConfig { api_key: "new-key".to_string(), model: "new-model".to_string() };
        providers.update_preset(PresetProvider::Gemini, config);
        let (_, api_key, model) = providers.raw_fields("gemini").unwrap();
        assert_eq!(api_key, "new-key");
        assert_eq!(model, "new-model");
    }

    #[test]
    fn ensure_all_presets_fills_missing() {
        let mut providers = LlmProviders::default();
        // Remove a preset entry from the inner map to simulate a stale config file.
        providers.presets.remove(&PresetProvider::Groq);
        assert!(providers.presets.get(&PresetProvider::Groq).is_none());
        let providers = providers.ensure_all_presets();
        assert!(providers.presets.get(&PresetProvider::Groq).is_some());
    }

    #[test]
    fn resolve_active_with_valid_preset() {
        let providers = LlmProviders::test_valid();
        let config = providers.resolve_active().unwrap();
        assert_eq!(config.base_url, PresetProvider::OpenAI.base_url());
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.model, "test-model");
    }

    #[test]
    fn resolve_active_with_empty_api_key_fails() {
        let providers = LlmProviders::default();
        let result = providers.resolve_active();
        assert!(matches!(
            result,
            Err(LlmConfigError::InvalidConfig(InvalidConfigReason::ApiKeyEmpty))
        ));
    }

    #[test]
    fn toml_round_trip() {
        let mut providers = LlmProviders::default();
        providers.update_preset(
            PresetProvider::OpenAI,
            PresetConfig { api_key: "sk-test".to_string(), model: "gpt-4o".to_string() },
        );
        providers
            .upsert_custom(
                "my-local".to_string(),
                CustomProvider {
                    base_url: "https://proxy.example.com/v1".to_string(),
                    api_key: "sk-proxy".to_string(),
                    model: "llama-3".to_string(),
                },
            )
            .unwrap();
        providers.set_active("my-local").unwrap();

        let toml_str = toml::to_string_pretty(&providers).unwrap();
        let restored: LlmProviders = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.active(), "my-local");
        assert_eq!(restored.raw_fields("openai").unwrap(), providers.raw_fields("openai").unwrap());
        assert_eq!(
            restored.raw_fields("my-local").unwrap(),
            providers.raw_fields("my-local").unwrap()
        );
    }

    #[test]
    fn preset_from_name_round_trip() {
        for preset in ALL_PRESETS {
            assert_eq!(PresetProvider::from_name(preset.name()), Some(*preset));
        }
        assert_eq!(PresetProvider::from_name("nonexistent"), None);
    }

    #[test]
    fn preset_display_matches_name() {
        for preset in ALL_PRESETS {
            assert_eq!(preset.to_string(), preset.name());
        }
    }

    #[test]
    fn provider_exists_covers_both_kinds() {
        let mut providers = LlmProviders::default();
        providers.upsert_custom("my-local".to_string(), CustomProvider::default()).unwrap();
        assert!(providers.provider_exists("openai"));
        assert!(providers.provider_exists("my-local"));
        assert!(!providers.provider_exists("nonexistent"));
    }

    #[test]
    fn upsert_custom_update_existing() {
        let mut providers = LlmProviders::default();
        providers.upsert_custom("my-local".to_string(), CustomProvider::default()).unwrap();
        let updated = CustomProvider {
            base_url: "https://updated.example.com/v1".to_string(),
            api_key: "new-key".to_string(),
            model: "new-model".to_string(),
        };
        providers.upsert_custom("my-local".to_string(), updated).unwrap();
        let (base_url, api_key, model) = providers.raw_fields("my-local").unwrap();
        assert_eq!(base_url, "https://updated.example.com/v1");
        assert_eq!(api_key, "new-key");
        assert_eq!(model, "new-model");
    }
}
