//! Error banner UI component.
//!
//! Renders a stacked list of error entries with per-entry dismiss buttons.
//! Uses theme constants; all user-facing text is i18n.

use crate::theme;
use iced::{
    Element,
    widget::{button, column, container, row, text},
};
use rust_i18n::t;

/// View-model for one prior error entry in the banner.
#[derive(Debug, Clone)]
pub struct ErrorBannerEntry {
    pub index: usize,
    pub message: String,
}

/// Data needed to render the error banner.
#[derive(Debug, Clone)]
pub struct ErrorBannerContent {
    pub title: String,
    pub latest_index: usize,
    pub previous_entries: Vec<ErrorBannerEntry>,
    pub hidden_previous_count: usize,
}

/// Renders the error banner from prepared content.
pub struct ErrorBannerView;

impl ErrorBannerView {
    /// Build the banner element. `on_dismiss` receives the error index to dismiss.
    pub fn view<'a, Message: Clone + 'a>(
        content: &ErrorBannerContent, on_dismiss: impl Fn(usize) -> Message + 'a,
    ) -> Element<'a, Message> {
        let mut banner_content = column![
            row![
                text(content.title.clone()),
                button(text(t!("ui_dismiss").to_string()))
                    .on_press(on_dismiss(content.latest_index)),
            ]
            .spacing(theme::PANEL_BUTTON_GAP)
            .align_y(iced::Alignment::Center)
        ]
        .spacing(theme::INLINE_GAP);

        for entry in &content.previous_entries {
            banner_content = banner_content.push(
                row![
                    text(t!("error_earlier", message = entry.message.as_str()).to_string()),
                    button(text(t!("ui_dismiss").to_string())).on_press(on_dismiss(entry.index)),
                ]
                .spacing(theme::PANEL_BUTTON_GAP)
                .align_y(iced::Alignment::Center),
            );
        }
        if content.hidden_previous_count > 0 {
            banner_content = banner_content.push(text(
                t!("error_older_count", count = content.hidden_previous_count).to_string(),
            ));
        }

        container(banner_content).style(theme::error_banner).padding(theme::BANNER_PAD).into()
    }
}
