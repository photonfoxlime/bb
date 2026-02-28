//! Shared text-only button constructors.
//!
//! This module centralizes text-button styling so action rows keep consistent
//! typography and button semantics across views.

use crate::theme;
use iced::{
    Color,
    widget::{button, text},
};

/// Namespace for text-only button constructors.
///
/// Invariant: standard text action buttons use `INTER` and route through shared
/// theme styles (`action_button`, `destructive_button`, `panel_toggle_button`).
pub(crate) struct TextButton;

impl TextButton {
    /// Build a standard action-style text button.
    pub(crate) fn action<'a, Message: 'a>(
        label: impl Into<String>, size: f32,
    ) -> button::Button<'a, Message> {
        button(text(label.into()).font(theme::INTER).size(size)).style(theme::action_button)
    }

    /// Build a standard destructive-style text button.
    pub(crate) fn destructive<'a, Message: 'a>(
        label: impl Into<String>, size: f32,
    ) -> button::Button<'a, Message> {
        button(text(label.into()).font(theme::INTER).size(size)).style(theme::destructive_button)
    }

    /// Build an action-style text button with explicit text color.
    pub(crate) fn action_with_color<'a, Message: 'a>(
        label: impl Into<String>, size: f32, color: Color,
    ) -> button::Button<'a, Message> {
        button(text(label.into()).font(theme::INTER).size(size).color(color))
            .style(theme::action_button)
    }

    /// Build a panel-toggle text button that reflects active/open state.
    pub(crate) fn panel_toggle<'a, Message: 'a>(
        label: impl Into<String>, size: f32, is_active: bool,
    ) -> button::Button<'a, Message> {
        button(text(label.into()).font(theme::INTER).size(size))
            .style(move |theme, status| theme::panel_toggle_button(theme, status, is_active))
    }
}
