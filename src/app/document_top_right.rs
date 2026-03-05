//! Document top-right toolbar component.
//!
//! Renders undo, redo, shortcut help, and settings buttons.
//! Uses theme constants; delegates to IconButton.

use super::{Message, settings::SettingsMessage};
use super::overlay::OverlayMessage;
use super::undo_redo::UndoRedoMessage;
use crate::component::icon_button::IconButton;
use crate::theme;
use iced::{Element, Fill};
use iced::widget::{container, row};
use lucide_icons::iced as icons;

/// View model for the top-right toolbar.
pub struct DocumentTopRightVm {
    pub can_undo: bool,
    pub can_redo: bool,
    pub show_shortcut_help: bool,
}

/// Renders the top-right button row (undo, redo, help, settings).
pub fn view<'a>(vm: DocumentTopRightVm) -> Element<'a, Message> {
    let mut undo_button = IconButton::action(
        icons::icon_undo_2()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
    );
    if vm.can_undo {
        undo_button = undo_button.on_press(Message::UndoRedo(UndoRedoMessage::Undo));
    }

    let mut redo_button = IconButton::action(
        icons::icon_redo_2()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
    );
    if vm.can_redo {
        redo_button = redo_button.on_press(Message::UndoRedo(UndoRedoMessage::Redo));
    }

    let help_button = IconButton::mode(
        icons::icon_circle_question_mark()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
        vm.show_shortcut_help,
    )
    .on_press(Message::Overlay(OverlayMessage::ToggleShortcutHelp));

    let gear_button = IconButton::action(
        icons::icon_settings()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
    )
    .on_press(Message::Settings(SettingsMessage::Open));

    let buttons =
        row![undo_button, redo_button, help_button, gear_button].spacing(theme::ACTION_GAP);

    container(
        container(buttons)
            .width(Fill)
            .align_y(iced::alignment::Vertical::Top)
            .align_x(iced::alignment::Horizontal::Right)
            .padding(
                iced::Padding::new(theme::PANEL_PAD_H)
                    .right(theme::CANVAS_PAD)
                    .bottom(theme::PANEL_PAD_H),
            ),
    )
    .width(Fill)
    .height(Fill)
    .into()
}
