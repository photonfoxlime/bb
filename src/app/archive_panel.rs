//! Archive panel: floating overlay for browsing and permanently deleting archived blocks.
//!
//! The panel is visible when [`DocumentMode::Archive`] is active. It lists every
//! block whose root id appears in [`BlockStore::archive`], showing the block's
//! point text and child count. Each archived row always shows restore-as-child
//! and restore-as-sibling actions; they are enabled only while a live block is
//! focused because the focus supplies the insertion target. Every item carries
//! a delete button that calls [`BlockStore::delete_archived_block`],
//! permanently destroying the subtree.
//!
//! Toggle the panel with the archive toolbar button (or `Message::Archive(Toggle)`).
//! Escape closes it via the global escape chain in `app.rs`.

use crate::app::{AppState, DocumentMode, Message};
use crate::component::floating_panel::{self, PanelHeader};
use crate::component::icon_button::IconButton;
use crate::text::truncate_for_display;
use crate::theme;
use iced::widget::{self, column, container, row, scrollable, text, tooltip};
use iced::{Alignment, Element, Length, Padding, Task};
use lucide_icons::iced as icons;
use rust_i18n::t;

/// Messages for archive panel interactions.
#[derive(Debug, Clone)]
pub enum ArchivePanelMessage {
    /// Toggle the archive panel: open if closed, close if open.
    Toggle,
    /// Close the archive panel without other side-effects.
    Close,
    /// Restore an archived block as the last child of the focused live block.
    RestoreAsChild(crate::store::BlockId),
    /// Restore an archived block immediately after the focused live block.
    RestoreAsSibling(crate::store::BlockId),
    /// Permanently destroy an archived block and its entire subtree.
    DeleteBlock(crate::store::BlockId),
}

/// Handle one archive-panel message.
pub fn handle(state: &mut AppState, message: ArchivePanelMessage) -> Task<Message> {
    match message {
        | ArchivePanelMessage::Toggle => {
            state.ui_mut().document_mode.toggle(DocumentMode::Archive);
            Task::none()
        }
        | ArchivePanelMessage::Close => {
            state.ui_mut().document_mode = DocumentMode::Normal;
            Task::none()
        }
        | ArchivePanelMessage::RestoreAsChild(block_id) => {
            let Some(target_id) = state.focus().map(|focus| focus.block_id) else {
                return Task::none();
            };

            state.snapshot_for_undo();
            if state.store.restore_archived_block_as_child(&block_id, &target_id).is_some() {
                state.editor_buffers.ensure_subtree(&state.store, &block_id);
                state.set_focus(block_id);
                tracing::info!(
                    block_id = ?block_id,
                    parent_block_id = ?target_id,
                    "restored archived block as child"
                );
                state.persist_with_context("after restoring archived block as child");
                let scroll = super::scroll::scroll_block_into_view(block_id);
                if let Some(widget_id) = state.editor_buffers.widget_id(&block_id) {
                    return Task::batch([widget::operation::focus(widget_id.clone()), scroll]);
                }
                return scroll;
            }
            Task::none()
        }
        | ArchivePanelMessage::RestoreAsSibling(block_id) => {
            let Some(target_id) = state.focus().map(|focus| focus.block_id) else {
                return Task::none();
            };

            state.snapshot_for_undo();
            if state.store.restore_archived_block_as_sibling(&block_id, &target_id).is_some() {
                state.editor_buffers.ensure_subtree(&state.store, &block_id);
                state.set_focus(block_id);
                tracing::info!(
                    block_id = ?block_id,
                    sibling_after_block_id = ?target_id,
                    "restored archived block as sibling"
                );
                state.persist_with_context("after restoring archived block as sibling");
                let scroll = super::scroll::scroll_block_into_view(block_id);
                if let Some(widget_id) = state.editor_buffers.widget_id(&block_id) {
                    return Task::batch([widget::operation::focus(widget_id.clone()), scroll]);
                }
                return scroll;
            }
            Task::none()
        }
        | ArchivePanelMessage::DeleteBlock(block_id) => {
            state.snapshot_for_undo();
            if let Some(removed_ids) = state.store.delete_archived_block(&block_id) {
                tracing::info!(
                    block_id = ?block_id,
                    removed = removed_ids.len(),
                    "permanently deleted archived block subtree"
                );
                state.persist_with_context("after deleting archived block");
            }
            Task::none()
        }
    }
}

/// Render the floating archive overlay.
///
/// Returns an invisible spacer when [`DocumentMode::Archive`] is not active so
/// the overlay participates in the `stack!` without consuming events.
pub fn floating_overlay<'a>(state: &'a AppState) -> Element<'a, Message> {
    if state.ui().document_mode != DocumentMode::Archive {
        return floating_panel::invisible_spacer();
    }

    let title = text(t!("ui_archive").to_string()).font(theme::INTER).size(theme::FIND_TITLE_SIZE);
    let close_btn = tooltip(
        IconButton::panel_close().on_press(Message::Archive(ArchivePanelMessage::Close)),
        text(t!("ui_close").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
        tooltip::Position::Bottom,
    )
    .style(theme::tooltip)
    .padding(theme::TOOLTIP_PAD)
    .gap(theme::TOOLTIP_GAP);
    let header = PanelHeader::new(title, close_btn);

    let archive_ids = state.store.archive().to_vec();
    let focused_block_id = state.focus().map(|focus| focus.block_id);

    let content: Element<'a, Message> = if archive_ids.is_empty() {
        text(t!("archive_empty").to_string())
            .size(theme::FIND_RESULT_META_SIZE)
            .style(theme::spine_text)
            .into()
    } else {
        let mut rows = column![].spacing(theme::PANEL_INNER_GAP);
        for block_id in &archive_ids {
            let point = state.store.point(block_id).unwrap_or_default();
            let label = truncate_for_display(&point, theme::FIND_RESULT_POINT_TRUNCATE);
            let child_count = state.store.children(block_id).len();

            let mut row_content = column![]
                .spacing(theme::FIND_RESULT_LINE_GAP)
                .push(text(label).font(theme::LXGW_WENKAI).size(theme::FIND_RESULT_POINT_SIZE));
            if child_count > 0 {
                row_content = row_content.push(
                    text(t!("archive_child_count", count = child_count).to_string())
                        .font(theme::INTER)
                        .size(theme::FIND_RESULT_META_SIZE)
                        .style(theme::spine_text),
                );
            }

            let restore_child_btn = {
                let button = IconButton::action_with_size(
                    icons::icon_corner_down_right().size(theme::FIND_CONTROL_ICON_SIZE).into(),
                    theme::FIND_CONTROL_BUTTON_SIZE,
                    theme::FIND_CONTROL_BUTTON_PAD,
                );
                let button = if focused_block_id.is_some() {
                    button
                        .on_press(Message::Archive(ArchivePanelMessage::RestoreAsChild(*block_id)))
                } else {
                    button
                };
                tooltip(
                    button,
                    text(t!("archive_restore_as_child").to_string())
                        .size(theme::SMALL_TEXT_SIZE)
                        .font(theme::INTER),
                    tooltip::Position::Bottom,
                )
                .style(theme::tooltip)
                .padding(theme::TOOLTIP_PAD)
                .gap(theme::TOOLTIP_GAP)
            };
            let restore_sibling_btn = {
                let button = IconButton::action_with_size(
                    icons::icon_plus().size(theme::FIND_CONTROL_ICON_SIZE).into(),
                    theme::FIND_CONTROL_BUTTON_SIZE,
                    theme::FIND_CONTROL_BUTTON_PAD,
                );
                let button = if focused_block_id.is_some() {
                    button.on_press(Message::Archive(ArchivePanelMessage::RestoreAsSibling(
                        *block_id,
                    )))
                } else {
                    button
                };
                tooltip(
                    button,
                    text(t!("archive_restore_as_sibling").to_string())
                        .size(theme::SMALL_TEXT_SIZE)
                        .font(theme::INTER),
                    tooltip::Position::Bottom,
                )
                .style(theme::tooltip)
                .padding(theme::TOOLTIP_PAD)
                .gap(theme::TOOLTIP_GAP)
            };
            let mut actions = row![restore_child_btn, restore_sibling_btn]
                .spacing(theme::FLOATING_PANEL_CONTROL_GAP);

            let delete_btn = tooltip(
                IconButton::destructive_with_size(
                    icons::icon_trash_2().size(theme::FIND_CONTROL_ICON_SIZE).into(),
                    theme::FIND_CONTROL_BUTTON_SIZE,
                    theme::FIND_CONTROL_BUTTON_PAD,
                )
                .on_press(Message::Archive(ArchivePanelMessage::DeleteBlock(*block_id))),
                text(t!("doc_delete").to_string()).size(theme::SMALL_TEXT_SIZE).font(theme::INTER),
                tooltip::Position::Bottom,
            )
            .style(theme::tooltip)
            .padding(theme::TOOLTIP_PAD)
            .gap(theme::TOOLTIP_GAP);

            actions = actions.push(delete_btn);

            rows = rows.push(
                container(
                    row![container(row_content).width(Length::Fill), actions,]
                        .align_y(Alignment::Center),
                )
                .padding(Padding::from([theme::FIND_RESULT_PAD_V, theme::FIND_RESULT_PAD_H]))
                .width(Length::Fill),
            );
        }
        scrollable(rows).height(Length::Fixed(theme::FIND_RESULT_LIST_HEIGHT)).into()
    };

    let viewport_width = state.ui().window_size.width;
    let viewport_height = state.ui().window_size.height;

    let content = column![].spacing(theme::FLOATING_PANEL_SECTION_GAP).push(header).push(content);

    floating_panel::wrap(content, viewport_width, viewport_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_as_child_reattaches_archive_under_focus() {
        let (mut state, root) = AppState::test_state();
        let archived =
            state.store.append_sibling(&root, "archived".to_string()).expect("sibling exists");
        let archived_ids = state.store.archive_block(&archived).expect("archive succeeds");
        state.editor_buffers.remove_blocks(&archived_ids);
        state.set_focus(root);

        let _ = handle(&mut state, ArchivePanelMessage::RestoreAsChild(archived));

        assert!(state.store.archive().is_empty());
        assert_eq!(state.store.children(&root), &[archived]);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(archived));
    }

    #[test]
    fn restore_as_sibling_reattaches_archive_after_focus() {
        let (mut state, root) = AppState::test_state();
        let archived =
            state.store.append_sibling(&root, "archived".to_string()).expect("sibling exists");
        let archived_ids = state.store.archive_block(&archived).expect("archive succeeds");
        state.editor_buffers.remove_blocks(&archived_ids);
        state.set_focus(root);

        let _ = handle(&mut state, ArchivePanelMessage::RestoreAsSibling(archived));

        assert!(state.store.archive().is_empty());
        assert_eq!(state.store.roots(), &[root, archived]);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(archived));
    }
}
