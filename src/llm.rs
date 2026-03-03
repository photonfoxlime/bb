//! LLM integration: configuration, prompt construction, and API client.
//!
//! # Module overview
//!
//! - [`config`] - Configuration types for LLM providers and endpoint settings.
//!   Key types: [`LlmConfig`], [`LlmProviders`], [`PresetProvider`], [`CustomProvider`], [`ApiStyle`].
//! - [`error`] - Error types for LLM runtime failures.
//! - [`context`] - Domain types and formatting for block context sent to the LLM.
//!   Key types: [`BlockContext`], [`Lineage`], [`FriendContext`], [`ExpandResult`], [`ReduceResult`], [`ContextFormatter`].
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
    PresetProvider,
};
pub use context::{
    BlockContext, ExpandResult, ExpandSuggestion, FriendContext, Lineage, ReduceResult,
};
