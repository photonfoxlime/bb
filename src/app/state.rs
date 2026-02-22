//! Async operation state types for the application layer.

use crate::llm;
use std::hash::{Hash, Hasher};

/// Type-erased error wrapper for display in the UI.
///
/// Wraps any error message as a string so the view layer does not depend
/// on concrete error types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiError {
    message: String,
}

impl UiError {
    pub(crate) fn from_message(message: impl ToString) -> Self {
        Self { message: message.to_string() }
    }

    pub(crate) fn as_str(&self) -> &str {
        self.message.as_str()
    }
}

/// Categorized UI error: tracks which subsystem originated the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppError {
    Configuration(UiError),
    Persistence(UiError),
    Reduce(UiError),
    Expand(UiError),
    /// Error from a mount/unmount operation.
    Mount(UiError),
}

impl AppError {
    pub(crate) fn message(&self) -> &str {
        match self {
            | Self::Configuration(err)
            | Self::Persistence(err)
            | Self::Reduce(err)
            | Self::Expand(err)
            | Self::Mount(err) => err.as_str(),
        }
    }
}

/// Per-block reduce operation state: Idle → Loading → Idle/Error.
///
/// Stored in a map keyed by `BlockId`; missing entry means Idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReduceState {
    Idle,
    Loading,
    Error { reason: UiError },
}

impl Default for ReduceState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Per-block expand operation state: Idle → Loading → Idle/Error.
///
/// Stored in a map keyed by `BlockId`; missing entry means Idle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExpandState {
    Idle,
    Loading,
    Error { reason: UiError },
}

impl Default for ExpandState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Captured request-context fingerprint for async expand/reduce.
///
/// Built from full lineage (root-to-target points). Responses are applied only
/// when the current lineage fingerprint matches this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestSignature {
    hash: u64,
    item_count: usize,
}

impl RequestSignature {
    pub(crate) fn from_lineage(lineage: &llm::Lineage) -> Option<Self> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut item_count = 0usize;
        for point in lineage.points() {
            Self::text_signature(point).hash(&mut hasher);
            item_count += 1;
        }
        if item_count == 0 {
            return None;
        }
        Some(Self { hash: hasher.finish(), item_count })
    }

    fn text_signature(text: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // UiError tests
    #[test]
    fn ui_error_from_message_stores_text() {
        let err = UiError::from_message("oops");
        assert_eq!(err.as_str(), "oops");
    }

    #[test]
    fn ui_error_from_non_string() {
        let err = UiError::from_message(42);
        assert_eq!(err.as_str(), "42");
    }

    // AppError tests
    #[test]
    fn app_error_configuration_message() {
        let err = AppError::Configuration(UiError::from_message("cfg"));
        assert_eq!(err.message(), "cfg");
    }

    #[test]
    fn app_error_reduce_message() {
        let err = AppError::Reduce(UiError::from_message("sum"));
        assert_eq!(err.message(), "sum");
    }

    #[test]
    fn app_error_persistence_message() {
        let err = AppError::Persistence(UiError::from_message("persist"));
        assert_eq!(err.message(), "persist");
    }

    #[test]
    fn app_error_expand_message() {
        let err = AppError::Expand(UiError::from_message("exp"));
        assert_eq!(err.message(), "exp");
    }

    #[test]
    fn app_error_mount_message() {
        let err = AppError::Mount(UiError::from_message("mnt"));
        assert_eq!(err.message(), "mnt");
    }

    // ReductionState tests
    #[test]
    fn reduce_state_default_is_idle() {
        assert_eq!(ReduceState::default(), ReduceState::Idle);
    }

    // ExpandState tests
    #[test]
    fn expand_state_default_is_idle() {
        assert_eq!(ExpandState::default(), ExpandState::Idle);
    }

    #[test]
    fn request_signature_from_empty_lineage_is_none() {
        let lineage = llm::Lineage::from_points(vec![]);
        assert!(RequestSignature::from_lineage(&lineage).is_none());
    }

    #[test]
    fn request_signature_changes_when_lineage_changes() {
        let first = llm::Lineage::from_points(vec!["root".to_string(), "child".to_string()]);
        let second =
            llm::Lineage::from_points(vec!["root changed".to_string(), "child".to_string()]);
        assert_ne!(RequestSignature::from_lineage(&first), RequestSignature::from_lineage(&second));
    }
}
