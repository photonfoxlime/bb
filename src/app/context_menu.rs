//! Context menu handler for text editors.
//!
//! Provides right-click context menu functionality with standard text editing
//! actions: undo, redo, cut, copy, paste, and select all.

use super::*;
use iced::clipboard;

pub fn handle(state: &mut AppState, message: ContextMenuMessage) -> Task<Message> {
    match message {
        | ContextMenuMessage::Show { block_id, position } => {
            state.ui_mut().context_menu = Some((block_id, position));
            state.set_focus(block_id);
            Task::none()
        }
        | ContextMenuMessage::Hide => {
            state.ui_mut().context_menu = None;
            Task::none()
        }
        | ContextMenuMessage::Action(action) => {
            let Some((block_id, _)) = state.ui().context_menu else {
                tracing::warn!("context menu action without active menu");
                return Task::none();
            };

            state.ui_mut().context_menu = None;
            state.editor_buffers.ensure_block(&state.store, &block_id);

            match action {
                | ContextMenuAction::Undo => {
                    return undo_redo::handle(state, UndoRedoMessage::Undo);
                }
                | ContextMenuAction::Redo => {
                    return undo_redo::handle(state, UndoRedoMessage::Redo);
                }
                | ContextMenuAction::Cut => {
                    let Some(content) = state.editor_buffers.get_mut(&block_id) else {
                        return Task::none();
                    };
                    let selected_text = content.selection().unwrap_or_default();
                    if !selected_text.is_empty() {
                        content.perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                        state.store.update_point(&block_id, content.text());
                        state.persist_with_context("after cut");
                        state.editor_buffers.invalidate_token_cache(&block_id);
                    }
                    return clipboard::write(selected_text);
                }
                | ContextMenuAction::Copy => {
                    let Some(content) = state.editor_buffers.get(&block_id) else {
                        return Task::none();
                    };
                    let selected_text = content.selection().unwrap_or_default();
                    return clipboard::write(selected_text);
                }
                | ContextMenuAction::Paste => {
                    return clipboard::read().then(move |text_opt| {
                        let Some(clipboard_text) = text_opt else {
                            return Task::done(Message::ContextMenu(ContextMenuMessage::Hide));
                        };
                        let action = text_editor::Action::Edit(text_editor::Edit::Paste(
                            std::sync::Arc::new(clipboard_text),
                        ));
                        Task::done(Message::Edit(EditMessage::PointEdited { block_id, action }))
                    });
                }
                | ContextMenuAction::SelectAll => {
                    let Some(content) = state.editor_buffers.get_mut(&block_id) else {
                        return Task::none();
                    };
                    content.perform(text_editor::Action::SelectAll);
                    Task::none()
                }
                | ContextMenuAction::ConvertToLink => {
                    state.store.toggle_to_link(&block_id);
                    // Rebuild editor buffer: link blocks show a chip, not a text editor.
                    state
                        .editor_buffers
                        .set_text(&block_id, &state.store.point(&block_id).unwrap_or_default());
                    state.persist_with_context("convert to link");
                    Task::none()
                }
                | ContextMenuAction::ConvertToText => {
                    state.store.toggle_to_text(&block_id);
                    state
                        .editor_buffers
                        .set_text(&block_id, &state.store.point(&block_id).unwrap_or_default());
                    state.persist_with_context("convert to text");
                    Task::none()
                }
            }
        }
    }
}
