//! Point editor widget for individual block rows.
//!
//! Renders the editable content of a single block point.
//!
//! Three rendering modes:
//! - `is_plain_text`: renders a static text container (friend-picker / multiselect).
//! - standard: renders an interactive `text_editor`.
//!
//! The component is generic over the application `Message` type. All
//! message construction is delegated to the caller via `fn` pointer
//! fields so the component carries no dependency on application-level
//! message enums.
//!
//! App-specific: couples to `BlockId` and the block document domain.
//!
//! # Key-binding layer
//!
//! The component's internal key-binding pipeline, executed for each key press
//! in the focused text editor:
//!
//! 1. Focus gate: non-focused editor instances pass the key press through
//!    as a standard binding so only one editor handles each structural chord.
//! 2. Caller shortcut: the `on_shortcut_key` callback is invoked first;
//!    if it returns `Some(msg)` that message is dispatched as a custom binding.
//! 3. Word cursor: `Option/Ctrl+Arrow` (platform-dependent) moves the
//!    cursor by one word token, dispatched via `on_word_move`.
//! 4. Fallback: the key press is forwarded to the default iced binding.
//!
//! # Editor shortcut routing
//!
//! See the top-level `document` module doc for the invariants around
//! structural Enter shortcuts and focused-editor-only dispatch.
//!
//! `shortcut_key()` resolves the shared structure shortcut inventory from
//! `shortcut.rs`, then applies one editor-specific rule: `AddChild` maps to the
//! dedicated `EditMessage::AddEmptyFirstChild` path so the focused editor owns
//! first-child insertion end to end.

use super::action_bar::ActionId;
use super::shortcut::action_shortcut_from_key;
use super::{ContextMenuMessage, EditMessage, Message, ShortcutMessage};
use crate::store::BlockId;
use crate::theme;
use iced::{
    Element, Fill, Length, Point,
    keyboard::{Key, key::Named},
    widget::{self, container, mouse_area, text, text_editor},
};
use rust_i18n::t;

/// Horizontal cursor movement direction for word-step shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordCursorDirection {
    Left,
    Right,
}

/// Point editor element for a single block row.
///
/// Construct with a struct literal and call [`PointTextEditor::view`] to
/// produce the element.
///
/// Two visual modes (selected by inspecting `is_plain_text`):
/// - `is_plain_text`: renders a static text container.
/// - standard: renders the interactive text editor.
pub struct PointTextEditor<'a, Message> {
    pub block_id: BlockId,
    pub is_plain_text: bool,
    pub point_text: String,
    pub editor_content: Option<&'a text_editor::Content>,
    pub widget_id: Option<&'a widget::Id>,
    pub cursor_position: Point,
    pub placeholder: String,
    /// Message to emit on a right-click anywhere in the editor.
    pub on_context_menu: fn(BlockId, Point) -> Message,
    /// Message wrapping a raw `text_editor::Action`.
    pub on_edit_action: fn(BlockId, text_editor::Action) -> Message,
    /// Message for Option/Ctrl+Arrow word-cursor movement.
    pub on_word_move: fn(BlockId, WordCursorDirection) -> Message,
    /// Called first in the key-binding pipeline; return `Some(msg)` to
    /// intercept the key press, `None` to fall through.
    pub on_shortcut_key: fn(BlockId, &text_editor::KeyPress) -> Option<Message>,
}

impl<'a, Message: Clone + 'static + 'a> PointTextEditor<'a, Message> {
    /// Consume the struct and produce the widget element.
    pub fn view(self) -> Element<'a, Message> {
        let Self {
            block_id,
            is_plain_text,
            point_text,
            editor_content,
            widget_id,
            cursor_position,
            placeholder,
            on_context_menu,
            on_edit_action,
            on_word_move,
            on_shortcut_key,
        } = self;

        if is_plain_text {
            // In friend picker or multiselect mode, render as plain text so
            // the block wrapper can capture clicks.
            return container(text(point_text)).width(Fill).height(Length::Shrink).into();
        }

        // Text editor — always rendered.
        // Safety: editor_content is always Some for non-plain-text blocks.
        let editor_content =
            editor_content.expect("editor_content must be Some for non-plain-text blocks");
        let key_binding = build_key_binding(block_id, on_word_move, on_shortcut_key);
        let mut editor = text_editor(editor_content)
            .placeholder(placeholder)
            .style(theme::point_editor)
            .on_action(move |action| on_edit_action(block_id, action))
            .key_binding(key_binding)
            .height(Length::Shrink);
        if let Some(wid) = widget_id {
            editor = editor.id(wid.clone());
        }
        let editor_el =
            mouse_area(editor).on_right_press(on_context_menu(block_id, cursor_position));

        editor_el.into()
    }
}

/// Build the text-editor key-binding closure for a single block row.
///
/// See the module doc for the four-step dispatch pipeline.
///
/// Exposed so the application-layer adapter can reproduce the same pipeline
/// in unit tests without duplicating the focus-gate and word-cursor logic.
pub fn build_key_binding<Message: Clone>(
    block_id: BlockId, on_word_move: fn(BlockId, WordCursorDirection) -> Message,
    on_shortcut_key: fn(BlockId, &text_editor::KeyPress) -> Option<Message>,
) -> impl Fn(text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    move |key_press| {
        // Only the focused editor should resolve structural shortcuts.
        // Other editor instances must ignore the key press so one chord
        // yields one mutation for the active block.
        if !matches!(key_press.status, text_editor::Status::Focused { .. }) {
            return text_editor::Binding::from_key_press(key_press);
        }

        // Caller-provided shortcut handler (e.g. ActionId resolution).
        if let Some(msg) = on_shortcut_key(block_id, &key_press) {
            return Some(text_editor::Binding::Custom(msg));
        }

        // Word cursor movement (Option/Ctrl+Arrow, platform-dependent).
        if let Some(direction) = word_cursor_direction_for_key_press(&key_press) {
            return Some(text_editor::Binding::Custom(on_word_move(block_id, direction)));
        }

        text_editor::Binding::from_key_press(key_press)
    }
}

/// Map a key press to a word-cursor direction, or `None` if the key chord
/// is not a word-movement shortcut on this platform.
///
/// - macOS: `Option+ArrowLeft/Right` (Alt modifier, no Cmd/Ctrl/Shift)
/// - Other: `Ctrl+ArrowLeft/Right` or `Cmd+ArrowLeft/Right` (no Alt/Shift)
pub fn word_cursor_direction_for_key_press(
    key_press: &text_editor::KeyPress,
) -> Option<WordCursorDirection> {
    let modifiers = key_press.modifiers;
    #[cfg(target_os = "macos")]
    if !modifiers.alt() || modifiers.command() || modifiers.control() || modifiers.shift() {
        return None;
    }
    #[cfg(not(target_os = "macos"))]
    if !(modifiers.command() || modifiers.control()) || modifiers.alt() || modifiers.shift() {
        return None;
    }

    match key_press.key {
        | Key::Named(Named::ArrowLeft) => Some(WordCursorDirection::Left),
        | Key::Named(Named::ArrowRight) => Some(WordCursorDirection::Right),
        | _ => None,
    }
}

/// Render the point editor element for a block row.
///
/// Delegates to [`PointTextEditor`] with application-specific message
/// constructors and keyboard shortcut handling.
pub(super) fn view<'a>(
    block_id: BlockId, is_plain_text: bool, point_text: String,
    editor_content: Option<&'a text_editor::Content>, widget_id: Option<&'a widget::Id>,
    cursor_position: Point,
) -> Element<'a, Message> {
    PointTextEditor {
        block_id,
        is_plain_text,
        point_text,
        editor_content,
        widget_id,
        cursor_position,
        placeholder: t!("doc_placeholder_point").to_string(),
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

/// Resolve a key press to an application message using the shared shortcut registry.
///
/// Returns `Some(msg)` when the key chord matches a known action, `None`
/// otherwise. Plugged into the component as the `on_shortcut_key` callback.
///
/// Note: `AddChild` is translated to [`EditMessage::AddEmptyFirstChild`]
/// instead of `ShortcutMessage::ForBlock` so Enter-based child creation remains
/// editor-owned and cannot race the global subscription.
fn shortcut_key(block_id: BlockId, key_press: &text_editor::KeyPress) -> Option<Message> {
    action_shortcut_from_key(key_press.key.clone(), key_press.modifiers).map(|action_id| {
        match action_id {
            | ActionId::AddChild => Message::Edit(EditMessage::AddEmptyFirstChild { block_id }),
            | _ => Message::Shortcut(ShortcutMessage::ForBlock { block_id, action_id }),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::AppState;
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
    /// that the [`PointTextEditor`] widget wires together at render time.
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
