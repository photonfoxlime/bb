//! Context-menu-specific style functions re-exported through [`crate::theme`].

use super::{
    CONTEXT_MENU_BUTTON_BORDER_RADIUS, CONTEXT_MENU_BUTTON_HOVER_OPACITY,
    CONTEXT_MENU_BUTTON_PRESSED_OPACITY, focused_palette,
};
use iced::widget::button;
use iced::{Color, Theme, border};

/// Context menu button with a minimal hover effect.
pub fn context_menu_button(theme: &Theme, status: button::Status) -> button::Style {
    let p = focused_palette(theme);
    match status {
        | button::Status::Active => button::Style {
            background: None,
            text_color: p.ink,
            border: border::rounded(CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Hovered => button::Style {
            background: Some(Color { a: CONTEXT_MENU_BUTTON_HOVER_OPACITY, ..p.ink }.into()),
            text_color: p.ink,
            border: border::rounded(CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Pressed => button::Style {
            background: Some(Color { a: CONTEXT_MENU_BUTTON_PRESSED_OPACITY, ..p.ink }.into()),
            text_color: p.ink,
            border: border::rounded(CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Disabled => button::Style {
            background: None,
            text_color: p.spine_light,
            border: border::rounded(CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
    }
}
