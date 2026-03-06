//! Scroll the document canvas so the focused block is visible in the viewport.
//!
//! Provides [`scroll_block_into_view`], a two-stage widget operation that:
//!
//! 1. Traverses the widget tree to find the block's container bounds in layout
//!    coordinates.
//! 2. Traverses again to find the document scrollable, computes the absolute
//!    scroll offset needed to center the block in the viewport, and applies it
//!    directly via the scrollable's internal state.
//!
//! Two passes are required because the scrollable is encountered before its
//! children in the widget tree traversal, so the block's position is not yet
//! known when the scrollable is first visited.
//!
//! If the block is already fully visible, no scrolling is applied.

use crate::store::BlockId;
use iced::{
    Rectangle, Task, Vector,
    advanced::widget::{
        Id,
        operate,
        operation::{Operation, Outcome, Scrollable},
        operation::scrollable::AbsoluteOffset,
    },
};

use super::Message;

/// Stable widget ID for the document canvas scrollable.
///
/// Assigned in `document.rs` to the main scrollable widget. Used by
/// [`scroll_block_into_view`] to locate the correct scrollable in the widget
/// tree.
pub fn document_scrollable_id() -> Id {
    Id::new("document-scrollable")
}

/// Stable container ID for a block row.
///
/// Derived deterministically from the block ID's display string. Used for
/// block-level layout (e.g. multiselect styling).
pub fn block_container_id(block_id: BlockId) -> Id {
    Id::from(format!("block-{block_id}"))
}

/// Stable container ID for a block's point text editor.
///
/// Used by [`scroll_block_into_view`] to locate the point editor bounds so
/// scrolling centers the editor (not the whole block row) in the viewport.
pub fn point_editor_container_id(block_id: BlockId) -> Id {
    Id::from(format!("point-editor-{block_id}"))
}

/// Returns a [`Task`] that scrolls the document canvas so the block's point
/// editor is visible in the viewport.
///
/// Targets the point editor (not the whole block row) so the text input area
/// is centered when focus changes. Runs a two-stage widget operation (see
/// module docs). If the point editor is already fully visible, no scroll.
pub fn scroll_block_into_view(block_id: BlockId) -> Task<Message> {
    operate(FindBlockBounds {
        target_id: point_editor_container_id(block_id),
        scrollable_id: document_scrollable_id(),
        bounds: None,
    })
    .discard()
}

/// Stage 1: traverse the widget tree to find the block container's layout
/// bounds.
///
/// When the container with [`FindBlockBounds::target_id`] is encountered,
/// its bounds are recorded. On finish, chains [`DoScroll`] with those bounds.
/// Logs a debug message if the block is not found (e.g., when it is off-screen
/// due to folding or navigation).
struct FindBlockBounds {
    target_id: Id,
    scrollable_id: Id,
    /// Recorded on the first matching container visit.
    bounds: Option<Rectangle>,
}

impl Operation<()> for FindBlockBounds {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if self.bounds.is_none() && id == Some(&self.target_id) {
            self.bounds = Some(bounds);
        }
    }

    fn finish(&self) -> Outcome<()> {
        match self.bounds {
            Some(block_bounds) => Outcome::Chain(Box::new(DoScroll {
                scrollable_id: self.scrollable_id.clone(),
                block_bounds,
            })),
            None => {
                tracing::debug!(
                    target_id = ?self.target_id,
                    "block container not found during scroll-into-view traversal"
                );
                Outcome::None
            }
        }
    }
}

/// Stage 2: find the document scrollable and scroll the block into view.
///
/// Uses the block bounds from stage 1 to compute the absolute vertical offset
/// that centers the block in the scrollable viewport. Skips scrolling when the
/// block is already fully visible within the viewport.
struct DoScroll {
    scrollable_id: Id,
    block_bounds: Rectangle,
}

impl Operation<()> for DoScroll {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        if id != Some(&self.scrollable_id) {
            return;
        }

        // Block y position relative to the content top.
        //
        // The scrollable's content layout starts at the same y coordinate as
        // the viewport (`content_bounds.y == bounds.y`), so subtracting gives
        // the block's offset within the content area.
        let block_y = self.block_bounds.y - content_bounds.y;
        let block_height = self.block_bounds.height;
        let viewport_height = bounds.height;

        // `translation.y` is the current absolute vertical scroll offset.
        let current_offset = translation.y;

        // Skip scrolling if the block is already fully within the visible
        // range; only scroll when necessary to avoid jarring viewport jumps
        // during small cursor movements within a single visible block.
        let visible_top = current_offset;
        let visible_bottom = current_offset + viewport_height;
        if block_y >= visible_top && block_y + block_height <= visible_bottom {
            return;
        }

        // Center the block vertically in the viewport.
        let desired = block_y - viewport_height / 2.0 + block_height / 2.0;
        let max_offset = (content_bounds.height - viewport_height).max(0.0);
        let clamped = desired.clamp(0.0, max_offset);

        tracing::debug!(
            block_y,
            block_height,
            current_offset,
            new_offset = clamped,
            "scrolling block into view"
        );
        state.scroll_to(AbsoluteOffset { x: None, y: Some(clamped) });
    }

    fn finish(&self) -> Outcome<()> {
        Outcome::None
    }
}
