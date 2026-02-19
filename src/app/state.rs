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
