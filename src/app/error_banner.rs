//! Error banner view-model for surfacing application errors.
//!
//! Please use or create constants in `theme.rs` for all UI numeric values
//! (sizes, padding, gaps, colors). Avoid hardcoding magic numbers in this module.
//!
//! All user-facing text must be internationalized via `rust_i18n::t!`. Never
//! hardcode UI strings; add keys to the locale files instead.
//!
//! Converts the raw `AppState::errors` stack into a display-ready structure
//! with a title, a preview of recent entries, and a hidden-count summary.
//! Assumes [`rust_i18n::set_locale`] has been set (e.g. at view start) before calling [`title`].

use super::AppState;
use super::error::AppError;

const ERROR_STACK_PREVIEW_LIMIT: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ErrorBannerEntry {
    pub index: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ErrorBanner {
    /// Key for the prefix string (e.g. "error", "error_recovery_mode").
    pub prefix_key: &'static str,
    pub latest: ErrorBannerEntry,
    pub previous_entries: Vec<ErrorBannerEntry>,
    pub hidden_previous_count: usize,
    pub total_count: usize,
}

impl ErrorBanner {
    pub fn from_state(state: &AppState) -> Option<Self> {
        let (latest_index, latest) = state.errors.iter().enumerate().next_back()?;
        let prefix_key = if state.persistence_blocked && matches!(latest, AppError::Persistence(_))
        {
            "error_recovery_mode"
        } else {
            "error"
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
            prefix_key,
            latest: ErrorBannerEntry { index: latest_index, message: latest.message().to_string() },
            previous_entries,
            hidden_previous_count,
            total_count: state.errors.len(),
        })
    }

    /// Localized title. Call only after locale is set (e.g. in view).
    pub fn title(&self) -> String {
        let prefix = rust_i18n::t!(self.prefix_key).to_string();
        if self.total_count == 1 {
            rust_i18n::t!(
                "error_title_single",
                prefix = prefix.as_str(),
                message = self.latest.message.as_str()
            )
            .to_string()
        } else {
            rust_i18n::t!(
                "error_title_multi",
                prefix = prefix.as_str(),
                total = self.total_count,
                message = self.latest.message.as_str()
            )
            .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{super::*, *};

    fn test_state() -> AppState {
        AppState::test_state().0
    }

    #[test]
    fn is_none_when_there_are_no_errors() {
        let state = test_state();
        assert!(ErrorBanner::from_state(&state).is_none());
    }

    #[test]
    fn uses_latest_error_and_total_count() {
        rust_i18n::set_locale("en-US");
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
        rust_i18n::set_locale("en-US");
        let mut state = test_state();
        state.persistence_blocked = true;
        state.errors.push(AppError::Persistence(UiError::from_message("persistence disabled")));

        let banner = ErrorBanner::from_state(&state).expect("banner should exist");
        assert_eq!(banner.title(), "Recovery mode: persistence disabled");
    }

    #[test]
    fn limits_previous_preview_and_reports_hidden_count() {
        rust_i18n::set_locale("en-US");
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
