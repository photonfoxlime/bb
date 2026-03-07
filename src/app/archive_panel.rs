//! Archive panel: floating overlay for browsing and permanently deleting archived blocks.
//!
//! The panel is visible when [`DocumentMode::Archive`] is active. It lists every
//! block whose root id appears in [`BlockStore::archive`], showing the block's
//! point text and child count. Each item carries a delete button that calls
//! [`BlockStore::delete_archived_block`], permanently destroying the subtree.
//!
//! Toggle the panel with the archive toolbar button (or `Message::Archive(Toggle)`).
//! Escape closes it via the global escape chain in `app.rs`.

use crate::app::{AppState, DocumentMode, Message};
use crate::component::floating_panel::{self, PanelHeader};
use crate::component::text_button::TextButton;
use crate::text::truncate_for_display;
use crate::theme;
use iced::widget::{column, container, row, scrollable, text};
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
    /// Permanently destroy an archived block and its entire subtree.
    DeleteBlock(crate::store::BlockId),
}

/// Handle one archive-panel message.
pub fn handle(state: &mut AppState, message: ArchivePanelMessage) -> Task<Message> {
    match message {
        | ArchivePanelMessage::Toggle => {
            if state.ui().document_mode == DocumentMode::Archive {
                state.ui_mut().document_mode = DocumentMode::Normal;
            } else {
                state.ui_mut().document_mode = DocumentMode::Archive;
            }
            Task::none()
        }
        | ArchivePanelMessage::Close => {
            state.ui_mut().document_mode = DocumentMode::Normal;
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
    let close_btn = TextButton::action(t!("ui_close").to_string(), theme::FIND_META_SIZE)
        .on_press(Message::Archive(ArchivePanelMessage::Close));
    let header = PanelHeader::new(title, close_btn);

    let archive_ids = state.store.archive().to_vec();

    let content: Element<'a, Message> = if archive_ids.is_empty() {
        container(text(t!("archive_empty").to_string()).style(theme::spine_text))
            .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
            .width(Length::Fill)
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

            let delete_btn = iced::widget::button(
                icons::icon_trash_2()
                    .size(theme::FIND_META_SIZE)
                    .line_height(iced::widget::text::LineHeight::Relative(1.0)),
            )
            .style(theme::action_button)
            .padding(theme::FIND_RESULT_PAD_H)
            .on_press(Message::Archive(ArchivePanelMessage::DeleteBlock(*block_id)));

            rows = rows.push(
                container(
                    row![container(row_content).width(Length::Fill), delete_btn,]
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

    let content = column![].spacing(theme::PANEL_INNER_GAP).push(header).push(content);

    floating_panel::wrap(content, viewport_width, viewport_height)
}
