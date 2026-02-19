use crate::graph::BlockId;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppError {
    Configuration(UiError),
    Summary(UiError),
    Expand(UiError),
}

impl AppError {
    pub(crate) fn message(&self) -> &str {
        match self {
            | Self::Configuration(err) | Self::Summary(err) | Self::Expand(err) => err.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SummaryState {
    Idle,
    Loading(BlockId),
    Error { block_id: BlockId, reason: UiError },
}

impl Default for SummaryState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExpandState {
    Idle,
    Loading(BlockId),
    Error { block_id: BlockId, reason: UiError },
}

impl Default for ExpandState {
    fn default() -> Self {
        Self::Idle
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
    fn app_error_summary_message() {
        let err = AppError::Summary(UiError::from_message("sum"));
        assert_eq!(err.message(), "sum");
    }

    #[test]
    fn app_error_expand_message() {
        let err = AppError::Expand(UiError::from_message("exp"));
        assert_eq!(err.message(), "exp");
    }

    // SummaryState tests
    #[test]
    fn summary_state_default_is_idle() {
        assert_eq!(SummaryState::default(), SummaryState::Idle);
    }

    // ExpandState tests
    #[test]
    fn expand_state_default_is_idle() {
        assert_eq!(ExpandState::default(), ExpandState::Idle);
    }
}
