//! Context menu item button component.
//!
//! Renders a single context menu row: icon + label, styled for menu use.

use crate::theme;
use iced::widget::{button, row, text};
use iced::{Color, Element, Length, Theme, border};
use lucide_icons::iced as icons;

/// Icon identifiers for context menu actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuIcon {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl ContextMenuIcon {
    fn to_element<'a, Message: 'a>(&self) -> Element<'a, Message> {
        let icon = match self {
            | Self::Undo => icons::icon_undo(),
            | Self::Redo => icons::icon_redo(),
            | Self::Cut => icons::icon_scissors(),
            | Self::Copy => icons::icon_copy(),
            | Self::Paste => icons::icon_clipboard_paste(),
            | Self::SelectAll => icons::icon_list(),
        };
        icon.size(theme::CONTEXT_MENU_ICON_SIZE).into()
    }
}

/// Builds a context menu style button with icon and label.
pub struct ContextMenuButton;

impl ContextMenuButton {
    /// Build a menu row button.
    pub fn view<'a, Message: Clone + 'a>(
        label: String, icon: ContextMenuIcon, on_press: Message,
    ) -> Element<'a, Message> {
        let icon_el = icon.to_element();
        button(
            row![icon_el, text(label).width(Length::Fill)]
                .spacing(theme::PANEL_BUTTON_GAP)
                .align_y(iced::Alignment::Center),
        )
        .padding(iced::Padding::from([theme::CONTEXT_MENU_PAD, theme::CONTEXT_MENU_BUTTON_PAD_H]))
        .style(context_menu_button_style)
        .on_press(on_press)
        .width(Length::Fill)
        .into()
    }
}

/// Context-menu row button style used only by [`ContextMenuButton`].
///
/// Note: this stays local to the component because the row chrome is an
/// implementation detail of this specific menu layout rather than a shared app
/// semantic surface.
fn context_menu_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme::focused_palette(theme);
    match status {
        | button::Status::Active => button::Style {
            background: None,
            text_color: p.ink,
            border: border::rounded(theme::CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Hovered => button::Style {
            background: Some(Color { a: theme::CONTEXT_MENU_BUTTON_HOVER_OPACITY, ..p.ink }.into()),
            text_color: p.ink,
            border: border::rounded(theme::CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Pressed => button::Style {
            background: Some(
                Color { a: theme::CONTEXT_MENU_BUTTON_PRESSED_OPACITY, ..p.ink }.into(),
            ),
            text_color: p.ink,
            border: border::rounded(theme::CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
        | button::Status::Disabled => button::Style {
            background: None,
            text_color: p.spine_light,
            border: border::rounded(theme::CONTEXT_MENU_BUTTON_BORDER_RADIUS).width(0),
            snap: false,
            ..Default::default()
        },
    }
}
