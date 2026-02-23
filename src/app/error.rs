//! Application-facing error types for UI and interaction flows.

/// Display-oriented error wrapper used across app modules.
///
/// Keeps error transport lightweight while preserving user-facing messages.
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

/// Category of failure surfaced by the app shell.
///
/// The variant indicates the subsystem so rendering and telemetry can distinguish
/// configuration, persistence, and LLM workflow failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppError {
    Configuration(UiError),
    Persistence(UiError),
    Reduce(UiError),
    Expand(UiError),
    Mount(UiError),
    Inquire(UiError),
}

impl AppError {
    pub(crate) fn message(&self) -> &str {
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

#[cfg(test)]
mod tests {
    use super::{AppError, UiError};

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
}
