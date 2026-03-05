//! Keyboard-shortcuts help banner component.
//!
//! Renders a bottom-right overlay listing all supported shortcuts and editing
//! gestures. Uses theme constants for layout; all user-facing text is i18n.

use crate::theme;
use iced::{
    Element, Length,
    widget::{button, column, container, row, rule, space, text},
};
use rust_i18n::t;

/// Renders the shortcut help banner.
///
/// Call only when the banner should be shown; the parent typically wraps
/// the result in `Option` based on visibility state.
pub struct ShortcutHelpBanner;

impl ShortcutHelpBanner {
    /// Build the banner element. Call `on_dismiss` when the user dismisses.
    pub fn view<'a, Message: Clone + 'a>(on_dismiss: Message) -> Element<'a, Message> {
        let title = row![
            text(t!("shortcut_help_title").to_string())
                .size(theme::SHORTCUT_HELP_TITLE_SIZE)
                .font(theme::INTER),
            space::horizontal(),
            button(text(t!("ui_dismiss").to_string()))
                .on_press(on_dismiss.clone())
                .padding(theme::BUTTON_PAD)
        ]
        .spacing(theme::ACTION_GAP)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);

        let global_section = column![
            text(t!("shortcut_help_section_global").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_find").to_string()).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_find_next").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_find_previous").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_undo").to_string()).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_redo").to_string()).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_global_escape").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        let structure_section = column![
            text(t!("shortcut_help_section_structure").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_amplify").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_distill").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_atomize").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_add_child").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_add_sibling").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_structure_accept_all").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        #[cfg(target_os = "macos")]
        let movement_word_cursor = t!("shortcut_help_movement_word_cursor_macos").to_string();
        #[cfg(not(target_os = "macos"))]
        let movement_word_cursor = t!("shortcut_help_movement_word_cursor").to_string();
        #[cfg(target_os = "macos")]
        let movement_focus = t!("shortcut_help_movement_focus_macos").to_string();
        #[cfg(not(target_os = "macos"))]
        let movement_focus = t!("shortcut_help_movement_focus").to_string();
        #[cfg(target_os = "macos")]
        let movement_reorder = t!("shortcut_help_movement_reorder_macos").to_string();
        #[cfg(not(target_os = "macos"))]
        let movement_reorder = t!("shortcut_help_movement_reorder").to_string();
        #[cfg(target_os = "macos")]
        let movement_outdent = t!("shortcut_help_movement_outdent_macos").to_string();
        #[cfg(not(target_os = "macos"))]
        let movement_outdent = t!("shortcut_help_movement_outdent").to_string();
        #[cfg(target_os = "macos")]
        let movement_indent = t!("shortcut_help_movement_indent_macos").to_string();
        #[cfg(not(target_os = "macos"))]
        let movement_indent = t!("shortcut_help_movement_indent").to_string();

        let movement_section = column![
            text(t!("shortcut_help_section_movement").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(movement_word_cursor).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(movement_focus).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(movement_reorder).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(movement_outdent).size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(movement_indent).size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        let backspace_section = column![
            text(t!("shortcut_help_section_backspace").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_backspace_enter_multiselect").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_backspace_delete_multiselect").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        let multiselect_section = column![
            text(t!("shortcut_help_section_multiselect").to_string())
                .font(theme::INTER)
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_multiselect_click").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_multiselect_shift_click").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
            text(t!("shortcut_help_multiselect_cmd_click").to_string())
                .size(theme::SHORTCUT_HELP_TEXT_SIZE),
        ]
        .spacing(theme::SHORTCUT_HELP_ROW_GAP);

        container(
            column![
                title,
                rule::horizontal(1),
                global_section,
                structure_section,
                movement_section,
                backspace_section,
                multiselect_section,
            ]
            .spacing(theme::SHORTCUT_HELP_SECTION_GAP),
        )
        .style(theme::shortcut_help_banner)
        .max_width(theme::SHORTCUT_HELP_MAX_WIDTH)
        .padding(theme::BANNER_PAD)
        .into()
    }
}
