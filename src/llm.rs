//! LLM integration: configuration, prompt construction, and API client.
//!
//! # Module overview
//!
//! - [`config`] - Configuration types for LLM providers and endpoint settings.
//!   Key types: [`LlmConfig`], [`LlmProviders`], [`PresetProvider`], [`CustomProvider`].
//! - [`error`] - Error types for LLM runtime failures.
//! - [`context`] - Domain types representing block context sent to the LLM.
//!   Key types: [`BlockContext`], [`Lineage`], [`FriendContext`], [`ExpandResult`], [`ReduceResult`].
//! - [`prompt`] - Prompt construction from block context.
//! - [`client`] - HTTP client for OpenAI-compatible endpoints.

pub mod client;
pub mod config;
pub mod context;
pub mod error;
pub mod prompt;

pub use client::LlmClient;
pub use config::{CustomProvider, LlmConfig, LlmProviders, PresetConfig, PresetProvider};
pub use context::{
    BlockContext, ExpandResult, ExpandSuggestion, FriendContext, Lineage, ReduceResult,
};
