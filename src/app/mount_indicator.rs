//! Mount header indicator component.
//!
//! Renders file path and mount actions (move, inline, inline-all).
//! Displayed above mount-backed nodes. Uses theme constants.

use super::mount_file::MountFileMessage;
use super::{Message, overlay::OverlayMessage};
use crate::component::icon_button::IconButton;
use crate::store::BlockId;
use crate::theme;
use iced::Element;
use iced::widget::{row, text, tooltip};
use lucide_icons::iced as icons;
use rust_i18n::t;
use std::path::Path;

/// View model for the mount indicator.
pub struct MountIndicatorVm<'a> {
    pub block_id: BlockId,
    pub mount_path: &'a Path,
    pub is_inline_confirmation_armed: bool,
    pub is_overflow_open: bool,
}

/// Renders the mount header with path and action buttons.
pub fn view<'a>(vm: MountIndicatorVm<'a>) -> Element<'a, Message> {
    let move_btn: Element<'a, Message> = {
        let btn = IconButton::action(
            icons::icon_folder_input()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        )
        .on_press(Message::MountFile(MountFileMessage::MoveMount(vm.block_id)));
        tooltip(
            btn,
            text(t!("action_move_mount_file").to_string())
                .size(theme::SMALL_TEXT_SIZE)
                .font(theme::INTER),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip)
        .padding(theme::TOOLTIP_PAD)
        .gap(theme::TOOLTIP_GAP)
        .into()
    };

    let inline_btn: Element<'a, Message> = {
        let btn = IconButton::action(
            icons::icon_chevron_down()
                .size(theme::TOOLBAR_ICON_SIZE)
                .line_height(iced::widget::text::LineHeight::Relative(1.0))
                .into(),
        )
        .on_press(Message::MountFile(MountFileMessage::InlineMount(vm.block_id)));
        tooltip(
            btn,
            text(t!("action_inline_mount").to_string())
                .size(theme::SMALL_TEXT_SIZE)
                .font(theme::INTER),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip)
        .padding(theme::TOOLTIP_PAD)
        .gap(theme::TOOLTIP_GAP)
        .into()
    };

    let inline_all_label = if vm.is_inline_confirmation_armed {
        t!("action_confirm_inline_mount_all").to_string()
    } else {
        t!("action_inline_mount_all").to_string()
    };

    let inline_all_btn: Element<'a, Message> = {
        let btn = if vm.is_inline_confirmation_armed {
            IconButton::destructive(
                icons::icon_chevrons_down()
                    .size(theme::TOOLBAR_ICON_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0))
                    .into(),
            )
        } else {
            IconButton::action(
                icons::icon_chevrons_down()
                    .size(theme::TOOLBAR_ICON_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0))
                    .into(),
            )
        }
        .on_press(Message::MountFile(MountFileMessage::InlineMountAll(vm.block_id)));
        tooltip(
            btn,
            text(inline_all_label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip)
        .padding(theme::TOOLTIP_PAD)
        .gap(theme::TOOLTIP_GAP)
        .into()
    };

    let overflow_toggle_btn: Element<'a, Message> = {
        let (btn, tooltip_label) = if vm.is_overflow_open {
            (
                IconButton::close_with_size(
                    theme::TOOLBAR_ICON_SIZE,
                    theme::ICON_BUTTON_SIZE,
                    theme::BUTTON_PAD,
                )
                .on_press(Message::Overlay(
                    OverlayMessage::ToggleMountActionsOverflow(vm.block_id),
                )),
                t!("ui_close").to_string(),
            )
        } else {
            (
                IconButton::action(
                    icons::icon_ellipsis()
                        .size(theme::TOOLBAR_ICON_SIZE)
                        .line_height(iced::widget::text::LineHeight::Relative(1.0))
                        .into(),
                )
                .on_press(Message::Overlay(
                    OverlayMessage::ToggleMountActionsOverflow(vm.block_id),
                )),
                t!("ui_more").to_string(),
            )
        };
        tooltip(
            btn,
            text(tooltip_label).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip)
        .padding(theme::TOOLTIP_PAD)
        .gap(theme::TOOLTIP_GAP)
        .into()
    };

    let confirm_close_btn: Element<'a, Message> = {
        let btn = IconButton::close_with_size(
            theme::TOOLBAR_ICON_SIZE,
            theme::ICON_BUTTON_SIZE,
            theme::BUTTON_PAD,
        )
        .on_press(Message::MountFile(MountFileMessage::CancelInlineMountAllConfirm(vm.block_id)));
        tooltip(
            btn,
            text(t!("ui_close").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
            tooltip::Position::Bottom,
        )
        .style(theme::tooltip)
        .padding(theme::TOOLTIP_PAD)
        .gap(theme::TOOLTIP_GAP)
        .into()
    };

    let mut header = row![
        text(vm.mount_path.display().to_string())
            .font(theme::INTER)
            .size(theme::MOUNT_HEADER_TEXT_SIZE)
            .style(theme::spine_text),
        iced::widget::space::horizontal(),
    ]
    .spacing(theme::ACTION_GAP)
    .align_y(iced::Alignment::Center);

    if vm.is_inline_confirmation_armed {
        header = header.push(
            text(t!("mount_inline_confirm_hint").to_string())
                .font(theme::INTER)
                .size(theme::SMALL_TEXT_SIZE)
                .style(theme::spine_text),
        );
        header = header.push(inline_all_btn);
        header = header.push(confirm_close_btn);
        return header.into();
    }

    if vm.is_overflow_open {
        header = header.push(move_btn);
        header = header.push(inline_btn);
        header = header.push(inline_all_btn);
    }
    header = header.push(overflow_toggle_btn);
    header.into()
}
