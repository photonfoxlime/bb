//! Error banner view-model for surfacing application errors.
//!
//! Converts the raw `AppState::errors` stack into a display-ready structure
//! with a title, a preview of recent entries, and a hidden-count summary.

use super::error::AppError;
use super::AppState;

const ERROR_STACK_PREVIEW_LIMIT: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ErrorBannerEntry {
    pub index: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ErrorBanner {
    pub prefix: &'static str,
    pub latest: ErrorBannerEntry,
    pub previous_entries: Vec<ErrorBannerEntry>,
    pub hidden_previous_count: usize,
    pub total_count: usize,
}

impl ErrorBanner {
    pub fn from_state(state: &AppState) -> Option<Self> {
        let (latest_index, latest) = state.errors.iter().enumerate().last()?;
        let prefix = if state.persistence_blocked && matches!(latest, AppError::Persistence(_)) {
            "Recovery mode"
        } else {
            "Error"
        };
        let previous_entries = state
            .errors
            .iter()
            .enumerate()
            .rev()
            .skip(1)
            .take(ERROR_STACK_PREVIEW_LIMIT)
            .map(|(index, error)| ErrorBannerEntry { index, message: error.message().to_string() })
            .collect::<Vec<_>>();
        let hidden_previous_count = state.errors.len().saturating_sub(1 + previous_entries.len());
        Some(Self {
            prefix,
            latest: ErrorBannerEntry { index: latest_index, message: latest.message().to_string() },
            previous_entries,
            hidden_previous_count,
            total_count: state.errors.len(),
        })
    }

    pub fn title(&self) -> String {
        if self.total_count == 1 {
            format!("{}: {}", self.prefix, self.latest.message)
        } else {
            format!("{} ({} total): {}", self.prefix, self.total_count, self.latest.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm;
    use crate::store::BlockStore;
    use crate::undo::UndoHistory;
    use super::super::{
        EditorBuffers, InstructionPanel, LlmRequests, SettingsState, ViewMode,
    };
    use super::super::error::UiError;

    fn test_state() -> AppState {
        let store = BlockStore::default();
        let providers = llm::LlmProviders::test_valid();
        AppState {
            editor_buffers: EditorBuffers::from_store(&store),
            store,
            undo_history: UndoHistory::with_capacity(64),
            settings: SettingsState::from_providers(&providers),
            providers,
            errors: vec![],
            llm_requests: LlmRequests::new(),
            overflow_open_for: None,
            instruction_panel: InstructionPanel::new(),
            friend_picker_for: None,
            focused_block_id: None,
            panel_bar_state: None,
            editing_block_id: None,
            persistence_blocked: false,
            persistence_write_disabled: true,
            is_dark: false,
            active_view: ViewMode::default(),
        }
    }

    #[test]
    fn is_none_when_there_are_no_errors() {
        let state = test_state();
        assert!(ErrorBanner::from_state(&state).is_none());
    }

    #[test]
    fn uses_latest_error_and_total_count() {
        let mut state = test_state();
        state.errors.push(AppError::Reduce(UiError::from_message("reduce failed")));
        state.errors.push(AppError::Expand(UiError::from_message("expand failed")));

        let banner = ErrorBanner::from_state(&state).expect("banner should exist");
        assert_eq!(banner.title(), "Error (2 total): expand failed");
        assert_eq!(
            banner.previous_entries,
            vec![ErrorBannerEntry { index: 0, message: "reduce failed".to_string() }]
        );
        assert_eq!(banner.hidden_previous_count, 0);
    }

    #[test]
    fn uses_recovery_prefix_for_latest_persistence_error() {
        let mut state = test_state();
        state.persistence_blocked = true;
        state.errors.push(AppError::Persistence(UiError::from_message("persistence disabled")));

        let banner = ErrorBanner::from_state(&state).expect("banner should exist");
        assert_eq!(banner.title(), "Recovery mode: persistence disabled");
    }

    #[test]
    fn limits_previous_preview_and_reports_hidden_count() {
        let mut state = test_state();
        state.errors.push(AppError::Mount(UiError::from_message("m1")));
        state.errors.push(AppError::Mount(UiError::from_message("m2")));
        state.errors.push(AppError::Mount(UiError::from_message("m3")));
        state.errors.push(AppError::Mount(UiError::from_message("m4")));
        state.errors.push(AppError::Mount(UiError::from_message("m5")));

        let banner = ErrorBanner::from_state(&state).expect("banner should exist");
        assert_eq!(banner.title(), "Error (5 total): m5");
        assert_eq!(
            banner.previous_entries,
            vec![
                ErrorBannerEntry { index: 3, message: "m4".to_string() },
                ErrorBannerEntry { index: 2, message: "m3".to_string() },
            ]
        );
        assert_eq!(banner.hidden_previous_count, 2);
    }
}