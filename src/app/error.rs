//! Application-facing error types for UI and interaction flows.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.

/// Display-oriented error wrapper used across app modules.
///
/// Keeps error transport lightweight while preserving user-facing messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError {
    message: String,
}

impl UiError {
    pub fn from_message(message: impl ToString) -> Self {
        Self { message: message.to_string() }
    }

    pub fn as_str(&self) -> &str {
        self.message.as_str()
    }
}

/// Category of failure surfaced by the app shell.
///
/// The variant indicates the subsystem so rendering and telemetry can distinguish
/// configuration, persistence, and LLM workflow failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    Configuration(UiError),
    Persistence(UiError),
    Reduce(UiError),
    Expand(UiError),
    Mount(UiError),
    Inquire(UiError),
}

impl AppError {
    pub fn message(&self) -> &str {
        match self {
            | Self::Configuration(err)
            | Self::Persistence(err)
            | Self::Reduce(err)
            | Self::Expand(err)
            | Self::Mount(err)
            | Self::Inquire(err) => err.as_str(),
        }
    }
}

use super::{AppState, Message};
use iced::Task;

/// Messages for error banner interaction.
#[derive(Debug, Clone)]
pub enum ErrorMessage {
    DismissAt(usize),
}

pub fn handle(state: &mut AppState, message: ErrorMessage) -> Task<Message> {
    match message {
        | ErrorMessage::DismissAt(index) => {
            if index < state.errors.len() {
                state.errors.remove(index);
                tracing::info!(
                    dismissed_index = index,
                    remaining = state.errors.len(),
                    "dismissed app error"
                );
            }
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    fn test_state() -> (AppState, crate::store::BlockId) {
        AppState::test_state()
    }

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

    #[test]
    fn dismiss_error_message_removes_selected_entry() {
        let (mut state, _) = test_state();
        state.errors.push(AppError::Mount(UiError::from_message("m1")));
        state.errors.push(AppError::Expand(UiError::from_message("e2")));
        state.errors.push(AppError::Reduce(UiError::from_message("r3")));

        let _ = AppState::update(&mut state, Message::Error(ErrorMessage::DismissAt(1)));

        assert_eq!(state.errors.len(), 2);
        assert_eq!(state.errors[0].message(), "m1");
        assert_eq!(state.errors[1].message(), "r3");
    }

    #[test]
    fn dismiss_error_message_out_of_bounds_is_noop() {
        let (mut state, _) = test_state();
        state.errors.push(AppError::Mount(UiError::from_message("m1")));

        let _ = AppState::update(&mut state, Message::Error(ErrorMessage::DismissAt(99)));

        assert_eq!(state.errors.len(), 1);
        assert_eq!(state.errors[0].message(), "m1");
    }
}
