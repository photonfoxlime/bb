//! Document mode bar component.
//!
//! Renders top-left mode buttons (Normal, Find, Link, Multiselect, Archive)
//! and multiselect count badge. Uses theme constants; delegates to IconButton.

use super::archive_panel::ArchivePanelMessage;
use super::{DocumentMode, FindMessage, Message};
use crate::component::icon_button::IconButton;
use crate::theme;
use iced::widget::{container, row, text};
use iced::{Element, Fill, Padding};
use lucide_icons::iced as icons;
use rust_i18n::t;

/// View model for the document mode bar.
pub struct DocumentToolbarVm {
    pub document_mode: DocumentMode,
    pub multiselect_count: usize,
}

/// Renders the document mode bar (top-left).
pub fn view<'a>(vm: DocumentToolbarVm) -> Element<'a, Message> {
    let is_normal_mode = vm.document_mode == DocumentMode::Normal;
    let is_find_mode = vm.document_mode == DocumentMode::Find;
    let is_archive_mode = vm.document_mode == DocumentMode::Archive;
    let is_multiselect_mode = vm.document_mode == DocumentMode::Multiselect;

    let normal_mode_btn = IconButton::mode(
        icons::icon_mouse_pointer_2()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
        is_normal_mode,
    )
    .on_press(Message::DocumentMode(DocumentMode::Normal));

    let find_mode_btn = IconButton::mode(
        icons::icon_search()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
        is_find_mode,
    )
    .on_press(Message::Find(if is_find_mode { FindMessage::Close } else { FindMessage::Open }));

    let archive_mode_btn = IconButton::mode(
        icons::icon_archive()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
        is_archive_mode,
    )
    .on_press(Message::Archive(ArchivePanelMessage::Toggle));

    let multiselect_mode_btn = IconButton::mode(
        icons::icon_square_check()
            .size(theme::TOOLBAR_ICON_SIZE)
            .line_height(iced::widget::text::LineHeight::Relative(1.0))
            .into(),
        is_multiselect_mode,
    )
    .on_press(Message::DocumentMode(if is_multiselect_mode {
        DocumentMode::Normal
    } else {
        DocumentMode::Multiselect
    }));

    let multiselect_badge: Element<'a, Message> = if is_multiselect_mode && vm.multiselect_count > 0
    {
        container(
            text(t!("multiselect_count", count = vm.multiselect_count).to_string())
                .size(theme::SMALL_TEXT_SIZE)
                .style(theme::status_text),
        )
        .padding(Padding::new(theme::CHIP_PAD_V).horizontal(theme::CHIP_PAD_H))
        .into()
    } else {
        Element::from(iced::widget::Space::new())
    };

    let toolbar = row![
        normal_mode_btn,
        find_mode_btn,
        archive_mode_btn,
        multiselect_mode_btn,
        multiselect_badge,
    ]
    .spacing(theme::ACTION_GAP)
    .align_y(iced::Alignment::Center);

    container(toolbar)
        .align_y(iced::alignment::Vertical::Top)
        .align_x(iced::alignment::Horizontal::Left)
        .padding(
            iced::Padding::new(theme::PANEL_PAD_H).left(theme::CANVAS_PAD).top(theme::CANVAS_TOP),
        )
        .width(Fill)
        .height(Fill)
        .into()
}
