//! LLM integration: configuration, prompt construction, and API client.
//!
//! # Module overview
//!
//! - [`config`] - Configuration types for LLM providers and endpoint settings.
//!   Key types: [`LlmConfig`], [`LlmProviders`], [`PresetProvider`], [`CustomProvider`], [`ApiStyle`].
//! - [`error`] - Error types for LLM runtime failures.
//! - [`context`] - Domain types and formatting for block context sent to the LLM.
//! - [`prompt`] - Prompt construction from block context.
//! - [`client`] - HTTP client for OpenAI-compatible and Anthropic endpoints.

pub mod client;
pub mod config;
pub mod context;
pub mod error;
pub mod prompt;

pub use client::{InquireStreamEvent, LlmClient};
pub use config::{
    ApiStyle, CustomProvider, DEFAULT_PROVIDER, LlmConfig, LlmProviders, PresetConfig,
    PresetProvider, TaskKind,
};
pub use context::{
    AtomizeResult, BlockContext, ChildrenContext, ContextFormatter, ExpandResult, ExpandSuggestion,
    FriendContext, LineageContext, ReduceResult,
};
pub use prompt::{TaskPromptConfig, default_system_prompt_hint, default_user_prompt_hint};
