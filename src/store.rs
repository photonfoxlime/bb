//! Application-facing block-store facade.
//!
//! The canonical persisted document model lives in the
//! `blooming-blockery-store` crate. This module keeps the existing
//! `crate::store::*` import surface stable for the GUI, CLI, and tests, and it
//! adds app-local navigation and LLM-context helpers through
//! [`BlockStoreNavigateExt`].

mod navigate;

pub use blooming_blockery_store::*;
pub use navigate::BlockStoreNavigateExt;

#[cfg(test)]
mod tests;
