//! Point editor widget for individual block rows.
//!
//! Renders the editable content of a single block point, which can be in one
//! of three states:
//! - **Plain text**: read-only display used in friend-picker or multiselect mode
//! - **Link chip**: a clickable chip showing a filesystem link with optional inline preview
//! - **Text editor**: an interactive `iced::text_editor` for freeform text input
//!
//! Key-binding resolution logic lives here alongside the view function so that
//! shortcut routing and visual rendering remain co-located.
//!
//! # Editor Shortcut Routing
//!
//! See the top-level `document` module doc for the invariants around
//! structural Enter shortcuts and focused-editor-only dispatch.

use super::{ContextMenuMessage, EditMessage, Message, ShortcutMessage};
use super::action_bar::{ActionId, shortcut_to_action};
use super::edit::WordCursorDirection;
use crate::store::{BlockId, LinkKind, PointContent};
use crate::theme;
use iced::{
    Element, Fill, Length, Point,
    widget::{self, button, column, container, mouse_area, row, text, text_editor},
};
use lucide_icons::iced as icons;
use rust_i18n::t;

/// Render the point editor element for a block row.
///
/// Three visual modes:
/// - `is_plain_text = true`: renders a static text container (used in
///   friend-picker and multiselect modes so the row wrapper captures clicks).
/// - link block: renders a clickable chip with optional inline preview.
/// - text block: renders an interactive `text_editor` with custom key bindings
///   and a right-click context menu.
pub(super) fn view<'a>(
    block_id: BlockId,
    is_plain_text: bool,
    point_text: String,
    point_content: Option<&'a PointContent>,
    editor_content: Option<&'a text_editor::Content>,
    widget_id: Option<&'a widget::Id>,
    cursor_position: Point,
    is_link_expanded: bool,
) -> Element<'a, Message> {
    if is_plain_text {
        // In friend picker or multiselect mode, render as plain text so the
        // block wrapper can capture clicks.
        container(text(point_text)).width(Fill).height(Length::Shrink).into()
    } else if let Some(link) = point_content.and_then(PointContent::as_link) {
        // Link point: render as a clickable chip instead of a text editor.
        let kind_icon: Element<'a, Message> = match link.kind {
            | LinkKind::Image => icons::icon_image().size(theme::LINK_CHIP_ICON_SIZE).into(),
            | LinkKind::Markdown => {
                icons::icon_file_text().size(theme::LINK_CHIP_ICON_SIZE).into()
            }
            | LinkKind::Path => icons::icon_link().size(theme::LINK_CHIP_ICON_SIZE).into(),
        };
        let label_text = link.display_text().to_owned();
        let chip = button(
            row![kind_icon, text(label_text).size(theme::LINK_CHIP_TEXT_SIZE)]
                .spacing(theme::LINK_CHIP_ICON_GAP)
                .align_y(iced::Alignment::Center),
        )
        .style(theme::link_chip_button)
        .padding(theme::LINK_CHIP_PAD)
        .on_press(Message::LinkChipToggle(block_id));

        let mut chip_col = column![mouse_area(chip).on_right_press(Message::ContextMenu(
            ContextMenuMessage::Show { block_id, position: cursor_position }
        ))];

        // Inline preview when expanded.
        if is_link_expanded {
            match link.kind {
                | LinkKind::Image => {
                    let img =
                        iced::widget::image(iced::widget::image::Handle::from_path(&link.href))
                            .width(Fill);
                    chip_col = chip_col.push(img);
                }
                | LinkKind::Markdown => {
                    // Read file contents and display as plain text.
                    let content_text = std::fs::read_to_string(&link.href)
                        .unwrap_or_else(|e| format!("(error: {})", e));
                    chip_col = chip_col.push(
                        container(text(content_text).size(theme::FIND_RESULT_POINT_SIZE))
                            .padding(theme::LINK_CHIP_PAD)
                            .width(Fill),
                    );
                }
                | LinkKind::Path => {
                    // No preview for generic paths.
                }
            }
        }

        chip_col.into()
    } else {
        // Safety: editor_content is always Some for non-link blocks (early
        // return above handles the None case).
        let editor_content =
            editor_content.expect("editor_content must be Some for text blocks");
        let mut editor = text_editor(editor_content)
            .placeholder(t!("doc_placeholder_point").to_string())
            .style(theme::point_editor)
            .on_action(move |action| {
                Message::Edit(EditMessage::PointEdited { block_id, action })
            })
            .key_binding(move |key_press| editor_key_binding(block_id, key_press))
            .height(Length::Shrink);
        if let Some(wid) = widget_id {
            editor = editor.id(wid.clone());
        }
        mouse_area(editor)
            .on_right_press(Message::ContextMenu(ContextMenuMessage::Show {
                block_id,
                position: cursor_position,
            }))
            .into()
    }
}

/// Resolve text-editor key bindings for one block row.
///
/// Structural Enter shortcuts are intentionally resolved here (instead of the
/// global subscription path) so they can target the exact focused block and be
/// dispatched exactly once.
fn editor_key_binding(
    block_id: BlockId, key_press: text_editor::KeyPress,
) -> Option<text_editor::Binding<Message>> {
    // Only the focused editor should resolve structural shortcuts.
    // Other editor instances must ignore the key press so one chord yields one
    // mutation for the active block.
    if !matches!(key_press.status, text_editor::Status::Focused { .. }) {
        return text_editor::Binding::from_key_press(key_press);
    }

    if let Some(action_id) = shortcut_to_action(key_press.key.clone(), key_press.modifiers) {
        // Design decision:
        // - `Cmd/Ctrl+Enter` uses a dedicated edit message so add-child behavior
        //   does not depend on asynchronous modifier-state updates.
        // - `Cmd/Ctrl+Shift+Enter` stays on shortcut dispatch so sibling
        //   insertion uses the same action semantics as the action bar.
        // - Shortcut dispatch is restricted to the focused editor above so a
        //   single keypress cannot fan out to every visible editor widget.
        return match action_id {
            | ActionId::AddChild => {
                Some(text_editor::Binding::Custom(Message::Edit(EditMessage::AddEmptyFirstChild {
                    block_id,
                })))
            }
            | ActionId::AddSibling => {
                Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                    block_id,
                    action_id,
                })))
            }
            | _ => {
                Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                    block_id,
                    action_id,
                })))
            }
        };
    }

    if let Some(direction) = word_cursor_direction_for_key_press(&key_press) {
        return Some(text_editor::Binding::Custom(Message::Edit(EditMessage::MoveCursorByWord {
            block_id,
            direction,
        })));
    }

    text_editor::Binding::from_key_press(key_press)
}

fn word_cursor_direction_for_key_press(
    key_press: &text_editor::KeyPress,
) -> Option<WordCursorDirection> {
    let modifiers = key_press.modifiers;
    if !(modifiers.command() || modifiers.control()) || modifiers.alt() || modifiers.shift() {
        return None;
    }

    match key_press.key {
        | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft) => {
            Some(WordCursorDirection::Left)
        }
        | iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight) => {
            Some(WordCursorDirection::Right)
        }
        | _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::AppState;

    fn enter_key_press(modifiers: iced::keyboard::Modifiers) -> text_editor::KeyPress {
        text_editor::KeyPress {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Enter),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    fn arrow_key_press(
        named: iced::keyboard::key::Named, code: iced::keyboard::key::Code,
        modifiers: iced::keyboard::Modifiers,
    ) -> text_editor::KeyPress {
        text_editor::KeyPress {
            key: iced::keyboard::Key::Named(named),
            modified_key: iced::keyboard::Key::Named(named),
            physical_key: iced::keyboard::key::Physical::Code(code),
            modifiers,
            text: None,
            status: text_editor::Status::Focused { is_hovered: false },
        }
    }

    #[test]
    fn command_enter_maps_to_add_empty_first_child_edit_message() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(root, enter_key_press(iced::keyboard::Modifiers::COMMAND));

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Edit(
                EditMessage::AddEmptyFirstChild { block_id }
            ))) if block_id == root
        ));
    }

    #[test]
    fn command_shift_enter_maps_to_add_sibling_shortcut() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            enter_key_press(iced::keyboard::Modifiers::COMMAND | iced::keyboard::Modifiers::SHIFT),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                block_id,
                action_id: ActionId::AddSibling,
            }))) if block_id == root
        ));
    }

    #[test]
    fn ctrl_shift_enter_maps_to_add_sibling_shortcut() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            enter_key_press(iced::keyboard::Modifiers::CTRL | iced::keyboard::Modifiers::SHIFT),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Shortcut(ShortcutMessage::ForBlock {
                block_id,
                action_id: ActionId::AddSibling,
            }))) if block_id == root
        ));
    }

    #[test]
    fn command_shift_enter_ignores_non_focused_editor() {
        let (_, root) = AppState::test_state();

        let mut key_press =
            enter_key_press(iced::keyboard::Modifiers::COMMAND | iced::keyboard::Modifiers::SHIFT);
        key_press.status = text_editor::Status::Active;

        let binding = editor_key_binding(root, key_press);

        assert!(binding.is_none());
    }

    #[test]
    fn command_left_maps_to_word_left_edit_message() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            arrow_key_press(
                iced::keyboard::key::Named::ArrowLeft,
                iced::keyboard::key::Code::ArrowLeft,
                iced::keyboard::Modifiers::COMMAND,
            ),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Edit(EditMessage::MoveCursorByWord {
                block_id,
                direction: WordCursorDirection::Left,
            }))) if block_id == root
        ));
    }

    #[test]
    fn ctrl_right_maps_to_word_right_edit_message() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            arrow_key_press(
                iced::keyboard::key::Named::ArrowRight,
                iced::keyboard::key::Code::ArrowRight,
                iced::keyboard::Modifiers::CTRL,
            ),
        );

        assert!(matches!(
            binding,
            Some(text_editor::Binding::Custom(Message::Edit(EditMessage::MoveCursorByWord {
                block_id,
                direction: WordCursorDirection::Right,
            }))) if block_id == root
        ));
    }
}
