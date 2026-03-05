//! Point editor widget for individual block rows.
//!
//! Renders the editable content of a single block point. A point always has
//! a text editor, optionally preceded by a row of link chips (one per entry
//! in the block's `links` vec).
//!
//! Three rendering modes:
//! - `is_plain_text`: renders a static text container (friend-picker / multiselect).
//! - standard: renders link chips (if any) above an interactive `text_editor`.
//!
//! # Link chip layout
//!
//! Link chips are rendered in a wrapping column above the text editor.
//! Each chip shows a kind icon and the link's display text. An inline
//! preview (image or markdown content) appears below the chip when expanded.
//! An × button on each chip removes that link from the block's point.
//!
//! The component is generic over the application `Message` type. All
//! message construction is delegated to the caller via `fn` pointer
//! fields so the component carries no dependency on application-level
//! message enums.
//!
//! App-specific: couples to store types (BlockId, PointLink, LinkKind)
//! and the block document domain.
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

use crate::store::{BlockId, LinkKind, PointLink};
use crate::theme;
use iced::{
    Element, Fill, Length, Point,
    keyboard::{Key, key::Named},
    widget::{self, button, column, container, mouse_area, row, text, text_editor},
};
use lucide_icons::iced as icons;

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
/// - standard: renders link chips (if any) above the interactive text editor.
pub struct PointTextEditor<'a, Message> {
    pub block_id: BlockId,
    pub is_plain_text: bool,
    pub point_text: String,
    /// Links attached to this block's point, rendered as chips above the editor.
    pub links: &'a [PointLink],
    pub editor_content: Option<&'a text_editor::Content>,
    pub widget_id: Option<&'a widget::Id>,
    pub cursor_position: Point,
    /// Which link chip index is currently expanded (showing inline preview), if any.
    pub expanded_link_index: Option<usize>,
    pub placeholder: String,
    /// Message to emit when a link chip is pressed (toggle expand).
    ///
    /// Arguments: `(block_id, link_index)`.
    pub on_link_chip_toggle: fn(BlockId, usize) -> Message,
    /// Message to emit when the × button on a chip is pressed.
    ///
    /// Arguments: `(block_id, link_index)`.
    pub on_remove_link: fn(BlockId, usize) -> Message,
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

impl<'a, Message: Clone + 'a> PointTextEditor<'a, Message> {
    /// Consume the struct and produce the widget element.
    pub fn view(self) -> Element<'a, Message> {
        let Self {
            block_id,
            is_plain_text,
            point_text,
            links,
            editor_content,
            widget_id,
            cursor_position,
            expanded_link_index,
            placeholder,
            on_link_chip_toggle,
            on_remove_link,
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

        // Build link chips column (empty when no links attached).
        let mut outer_col = column![].width(Fill);

        for (i, link) in links.iter().enumerate() {
            let kind_icon: Element<'a, Message> = match link.kind {
                | LinkKind::Image => icons::icon_image().size(theme::LINK_CHIP_ICON_SIZE).into(),
                | LinkKind::Markdown => {
                    icons::icon_file_text().size(theme::LINK_CHIP_ICON_SIZE).into()
                }
                | LinkKind::Path => icons::icon_link().size(theme::LINK_CHIP_ICON_SIZE).into(),
            };
            let label_text = link.display_text().to_owned();

            let expand_btn = button(
                row![kind_icon, text(label_text).size(theme::LINK_CHIP_TEXT_SIZE)]
                    .spacing(theme::LINK_CHIP_ICON_GAP)
                    .align_y(iced::Alignment::Center),
            )
            .style(theme::link_chip_button)
            .padding(theme::LINK_CHIP_PAD)
            .on_press(on_link_chip_toggle(block_id, i));

            let remove_btn = button(
                icons::icon_x()
                    .size(theme::LINK_CHIP_ICON_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0)),
            )
            .style(theme::link_chip_button)
            .padding(theme::LINK_CHIP_PAD)
            .on_press(on_remove_link(block_id, i));

            let chip_row = row![expand_btn, remove_btn]
                .spacing(theme::LINK_CHIP_ICON_GAP)
                .align_y(iced::Alignment::Center);

            let mut chip_col = column![chip_row];

            // Inline preview when this chip is expanded.
            if expanded_link_index == Some(i) {
                match link.kind {
                    | LinkKind::Image => {
                        let img =
                            iced::widget::image(iced::widget::image::Handle::from_path(&link.href))
                                .width(Fill);
                        chip_col = chip_col.push(img);
                    }
                    | LinkKind::Markdown => {
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

            outer_col = outer_col.push(chip_col);
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

        outer_col.push(editor_el).into()
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
