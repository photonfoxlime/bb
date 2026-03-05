//! Application-layer adapter for the generic `PointTextEditor` component.
//!
//! Wires application-specific message types and keyboard shortcut resolution
//! (`shortcut_to_action` / `ActionId`) into the generic
//! [`super::point_text_editor::PointTextEditor`] widget.
//!
//! # Editor Shortcut Routing
//!
//! See the top-level `document` module doc for the invariants around
//! structural Enter shortcuts and focused-editor-only dispatch.

use super::action_bar::{ActionId, shortcut_to_action};
use super::point_text_editor::PointTextEditor;
#[cfg(test)]
use super::point_text_editor::WordCursorDirection;
use super::{ContextMenuMessage, EditMessage, Message, ShortcutMessage};
use crate::store::{BlockId, PointLink};
use iced::{
    Element, Point,
    widget::{self, text_editor},
};
use rust_i18n::t;

/// Render the point editor element for a block row.
///
/// Delegates to [`PointTextEditor`] with application-specific message
/// constructors and keyboard shortcut handling.
pub(super) fn view<'a>(
    block_id: BlockId, is_plain_text: bool, point_text: String, links: &'a [PointLink],
    editor_content: Option<&'a text_editor::Content>, widget_id: Option<&'a widget::Id>,
    cursor_position: Point, expanded_link_index: Option<usize>,
) -> Element<'a, Message> {
    PointTextEditor {
        block_id,
        is_plain_text,
        point_text,
        links,
        editor_content,
        widget_id,
        cursor_position,
        expanded_link_index,
        placeholder: t!("doc_placeholder_point").to_string(),
        on_link_chip_toggle: |bid, idx| Message::LinkChipToggle(bid, idx),
        on_remove_link: |bid, idx| Message::Edit(EditMessage::RemoveLink { block_id: bid, index: idx }),
        on_context_menu: |bid, position| {
            Message::ContextMenu(ContextMenuMessage::Show { block_id: bid, position })
        },
        on_edit_action: |bid, action| {
            Message::Edit(EditMessage::PointEdited { block_id: bid, action })
        },
        on_word_move: |bid, direction| {
            Message::Edit(EditMessage::MoveCursorByWord { block_id: bid, direction })
        },
        on_shortcut_key: shortcut_key,
    }
    .view()
}

/// Resolve a key press to an application message using `shortcut_to_action`.
///
/// Returns `Some(msg)` when the key chord matches a known action, `None`
/// otherwise. Plugged into the component as the `on_shortcut_key` callback.
fn shortcut_key(block_id: BlockId, key_press: &text_editor::KeyPress) -> Option<Message> {
    shortcut_to_action(key_press.key.clone(), key_press.modifiers).map(
        |action_id| match action_id {
            | ActionId::AddChild => Message::Edit(EditMessage::AddEmptyFirstChild { block_id }),
            | _ => Message::Shortcut(ShortcutMessage::ForBlock { block_id, action_id }),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::AppState;
    use super::super::point_text_editor::build_key_binding;
    use super::*;

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

    /// Reconstruct the full key-binding pipeline used by [`view`], for testing.
    ///
    /// Applies the same focus gate, shortcut dispatch, and word-cursor logic
    /// that the [`super::point_text_editor::PointTextEditor`] widget wires together at render time.
    fn editor_key_binding(
        block_id: BlockId, key_press: text_editor::KeyPress,
    ) -> Option<text_editor::Binding<Message>> {
        let on_word_move = |bid: BlockId, direction: WordCursorDirection| {
            Message::Edit(EditMessage::MoveCursorByWord { block_id: bid, direction })
        };
        build_key_binding(block_id, on_word_move, shortcut_key)(key_press)
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

    #[cfg(not(target_os = "macos"))]
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

    #[cfg(target_os = "macos")]
    #[test]
    fn option_left_maps_to_word_left_edit_message_on_macos() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            arrow_key_press(
                iced::keyboard::key::Named::ArrowLeft,
                iced::keyboard::key::Code::ArrowLeft,
                iced::keyboard::Modifiers::ALT,
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

    #[cfg(not(target_os = "macos"))]
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

    #[cfg(target_os = "macos")]
    #[test]
    fn option_right_maps_to_word_right_edit_message_on_macos() {
        let (_, root) = AppState::test_state();

        let binding = editor_key_binding(
            root,
            arrow_key_press(
                iced::keyboard::key::Named::ArrowRight,
                iced::keyboard::key::Code::ArrowRight,
                iced::keyboard::Modifiers::ALT,
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
