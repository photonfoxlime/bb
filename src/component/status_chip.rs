//! Status chip component for action bar feedback.
//!
//! Renders amplifying/distilling/atomizing status or draft-ready labels.
//! Uses theme constants for layout; label is passed in from the parent.

use crate::theme;
use iced::widget::{container, text};
use iced::{Element, Length, Padding};

/// Renders a small status chip showing operation state (loading, error, draft).
pub struct StatusChip;

impl StatusChip {
    /// Build the chip element. Call with a non-empty label; parent omits the chip when idle.
    pub fn view<'a, Message: 'a>(label: String) -> Element<'a, Message> {
        container(
            text(label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER).style(theme::status_text),
        )
        .padding(Padding::from([theme::CHIP_PAD_V, theme::CHIP_PAD_H]))
        .width(Length::Shrink)
        .into()
    }
}
