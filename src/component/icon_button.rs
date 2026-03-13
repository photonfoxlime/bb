//! Shared icon-only button constructors.
//!
//! This module centralizes the layout contract for icon buttons so views can
//! reuse the same footprint, padding, and alignment behavior.

use crate::theme;
use iced::{
    Element, Length,
    widget::{button, container},
};
use lucide_icons::iced as icons;

/// Namespace for icon-only button constructors.
///
/// Invariant: icon-only controls use square footprints and centered glyphs.
/// This keeps row controls, toolbars, and compact action clusters visually
/// consistent across the application.
pub struct IconButton;

impl IconButton {
    /// Build a standard action-style icon button with default square footprint.
    pub fn action<'a, Message: 'a>(icon: Element<'a, Message>) -> button::Button<'a, Message> {
        button(Self::frame(icon, theme::ICON_BUTTON_SIZE, theme::BUTTON_PAD))
            .style(theme::action_button)
            .padding(iced::Padding::ZERO)
            .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
    }

    /// Build a standard destructive-style icon button with default footprint.
    pub fn destructive<'a, Message: 'a>(icon: Element<'a, Message>) -> button::Button<'a, Message> {
        button(Self::frame(icon, theme::ICON_BUTTON_SIZE, theme::BUTTON_PAD))
            .style(theme::destructive_button)
            .padding(iced::Padding::ZERO)
            .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
    }

    /// Build a mode-selector icon button using mode active/inactive styling.
    pub fn mode<'a, Message: 'a>(
        icon: Element<'a, Message>, is_active: bool,
    ) -> button::Button<'a, Message> {
        button(Self::frame(icon, theme::ICON_BUTTON_SIZE, theme::BUTTON_PAD))
            .style(move |theme, status| theme::mode_button(theme, status, is_active))
            .padding(iced::Padding::ZERO)
            .width(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
    }

    /// Build a toggle-style icon button with custom footprint and padding.
    pub fn toggle_with_size<'a, Message: 'a>(
        icon: Element<'a, Message>, is_on: bool, button_size: f32, icon_padding: f32,
    ) -> button::Button<'a, Message> {
        button(Self::frame(icon, button_size, icon_padding))
            .style(theme::toggle_button(is_on))
            .padding(iced::Padding::ZERO)
            .width(Length::Fixed(button_size))
            .height(Length::Fixed(button_size))
    }

    /// Build an action-style icon button with custom footprint and padding.
    pub fn action_with_size<'a, Message: 'a>(
        icon: Element<'a, Message>, button_size: f32, icon_padding: f32,
    ) -> button::Button<'a, Message> {
        button(Self::frame(icon, button_size, icon_padding))
            .style(theme::action_button)
            .padding(iced::Padding::ZERO)
            .width(Length::Fixed(button_size))
            .height(Length::Fixed(button_size))
    }

    /// Build a standard close button with an `x` glyph and custom footprint.
    ///
    /// Note: centralizing this keeps close affordances visually consistent
    /// across floating panels, inline probe panels, and compact header rows.
    pub fn close_with_size<'a, Message: 'a>(
        icon_size: f32, button_size: f32, icon_padding: f32,
    ) -> button::Button<'a, Message> {
        button(Self::frame(
            icons::icon_x()
                .size(icon_size)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
            button_size,
            icon_padding,
        ))
        .style(theme::close_button)
        .padding(iced::Padding::ZERO)
        .width(Length::Fixed(button_size))
        .height(Length::Fixed(button_size))
    }

    /// Build the standard compact close control used by panel headers.
    pub fn panel_close<'a, Message: 'a>() -> button::Button<'a, Message> {
        Self::close_with_size(
            theme::FIND_CONTROL_ICON_SIZE,
            theme::FIND_CONTROL_BUTTON_SIZE,
            theme::FIND_CONTROL_BUTTON_PAD,
        )
    }

    /// Build a destructive-style icon button with custom footprint and padding.
    pub fn destructive_with_size<'a, Message: 'a>(
        icon: Element<'a, Message>, button_size: f32, icon_padding: f32,
    ) -> button::Button<'a, Message> {
        button(Self::frame(icon, button_size, icon_padding))
            .style(theme::destructive_button)
            .padding(iced::Padding::ZERO)
            .width(Length::Fixed(button_size))
            .height(Length::Fixed(button_size))
    }

    /// Center an icon glyph inside a square frame.
    fn frame<'a, Message: 'a>(
        icon: Element<'a, Message>, button_size: f32, icon_padding: f32,
    ) -> Element<'a, Message> {
        container(icon)
            .padding(icon_padding)
            .width(Length::Fixed(button_size))
            .height(Length::Fixed(button_size))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }
}
