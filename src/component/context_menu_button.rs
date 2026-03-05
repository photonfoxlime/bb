//! Context menu item button component.
//!
//! Renders a single context menu row: icon + label, styled for menu use.

use crate::theme;
use iced::widget::{button, row, text};
use iced::{Element, Length};
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
    ConvertToLink,
    ConvertToText,
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
            | Self::ConvertToLink => icons::icon_link(),
            | Self::ConvertToText => icons::icon_type(),
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
        .style(theme::context_menu_button)
        .on_press(on_press)
        .width(Length::Fill)
        .into()
    }
}
