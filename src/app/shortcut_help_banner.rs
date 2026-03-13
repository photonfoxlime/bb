//! Keyboard-shortcuts help banner.
//!
//! Renders a bottom-right overlay listing all supported shortcuts and editing
//! gestures. Uses theme constants for layout; all user-facing text is i18n.
//!
//! The banner is a pure view over [`super::shortcut::ShortcutCatalog`], which
//! keeps the shortcut inventory typed and close to the runtime routing code.

use super::shortcut::ShortcutCatalog;
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

        let mut sections = column![title, rule::horizontal(theme::RULE_WIDTH)]
            .spacing(theme::SHORTCUT_HELP_SECTION_GAP);
        for section in ShortcutCatalog::banner_view_model() {
            let mut section_content = column![
                text(section.title).font(theme::INTER).size(theme::SHORTCUT_HELP_TEXT_SIZE)
            ]
            .spacing(theme::SHORTCUT_HELP_ROW_GAP);

            for row_vm in section.rows {
                section_content = section_content.push(
                    row![
                        container(
                            text(row_vm.chord)
                                .font(theme::INTER)
                                .size(theme::SHORTCUT_HELP_TEXT_SIZE)
                        )
                        .width(Length::Fixed(theme::SHORTCUT_HELP_CHORD_WIDTH))
                        .align_x(iced::alignment::Horizontal::Right),
                        text(row_vm.description)
                            .size(theme::SHORTCUT_HELP_TEXT_SIZE)
                            .width(Length::Fill),
                    ]
                    .spacing(theme::SHORTCUT_HELP_COLUMN_GAP)
                    .align_y(iced::Alignment::Start)
                    .width(Length::Fill),
                );
            }

            sections = sections.push(section_content);
        }

        container(sections)
            .style(theme::shortcut_help_banner)
            .max_width(theme::SHORTCUT_HELP_MAX_WIDTH)
            .padding(theme::BANNER_PAD)
            .into()
    }
}
