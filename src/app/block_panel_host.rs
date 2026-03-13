//! Inline panel host for focused block-local toggle panels.
//!
//! This module owns the chrome around block-local toggle panels such as
//! References: the toggle bar that appears below the block row and the toggle
//! panel body shown beneath it.
//!
//! Probe uses patch-style lifecycle semantics instead of toggle-panel
//! semantics, so it is rendered by `document.rs` alongside other inline draft
//! surfaces rather than through this host.
//!
//! ## Why This Host Is Intentionally Narrow
//!
//! The repository now distinguishes two UI lifecycle classes:
//! - persisted single-select toggle panels, currently References;
//! - transient draft-like panels, currently Probe plus LLM patch results.
//!
//! The earlier design pushed Probe through the same persisted enum slot as
//! References. That created coupling where opening one panel implicitly closed
//! the other because the host could only project one active panel body. The
//! current split is intentional: this host stays small and opinionated around
//! toggle panels, while draft-like panels are rendered by the document's inline
//! draft stack.
//!
//! Note: if a future panel should coexist with patch/probe-style surfaces and
//! only close from controls inside itself, it likely does not belong in this
//! host.
//!
//! Keeping this host separate from `document.rs` narrows the future merge seam
//! for reference-style panels. `DocumentView` can treat the panel area as one
//! composable block instead of owning panel-specific button wiring.

use super::{
    AppState, Message,
    reference_panel::{self, ReferencePanelMessage},
};
use crate::{
    component::text_button::TextButton,
    store::{BlockId, BlockPanelBarState},
    theme,
};
use iced::{
    Element, Length, Padding,
    widget::{column, container, row},
};
use rust_i18n::t;

/// Focused block-local toggle-panel host rendered below a block row.
///
/// The host is intentionally state-less beyond borrowed app state and the
/// current row identity. It only projects persisted toggle-panel selection and
/// emits panel-toggle messages.
///
/// Note: the host deliberately does not know how to render probe panels even
/// though `BlockPanelBarState` still carries a legacy `Probe` variant for
/// compatibility.
pub struct BlockPanelHost<'a> {
    state: &'a AppState,
    block_id: BlockId,
    is_focused: bool,
}

impl<'a> BlockPanelHost<'a> {
    /// Build a host for one block row.
    pub fn new(state: &'a AppState, block_id: BlockId, is_focused: bool) -> Self {
        Self { state, block_id, is_focused }
    }

    /// Render the panel toggle bar.
    ///
    /// Returns an empty element for non-focused rows because block-local panels
    /// are only interactive for the focused block.
    pub fn bar(&self) -> Element<'a, Message> {
        if !self.is_focused {
            return column![].into();
        }

        let references_panel_open = matches!(
            self.state.store.block_panel_state(&self.block_id),
            Some(BlockPanelBarState::References)
        );
        let button_row = row![].spacing(theme::PANEL_BUTTON_GAP).push(
            TextButton::panel_toggle(
                t!("ui_references").to_string(),
                theme::LABEL_TEXT_SIZE,
                references_panel_open,
            )
            .height(Length::Fixed(theme::ICON_BUTTON_SIZE))
            .on_press(Message::ReferencePanel(ReferencePanelMessage::Toggle(self.block_id))),
        );

        container(button_row).padding(Padding::ZERO.right(theme::INDENT)).into()
    }

    /// Render the currently visible toggle-panel body.
    ///
    /// Returns an empty element for non-focused rows or when no block-local
    /// toggle panel is open.
    pub fn body(&self) -> Element<'a, Message> {
        if !self.is_focused {
            return column![].into();
        }

        match self.state.store.block_panel_state(&self.block_id) {
            | Some(BlockPanelBarState::References) => {
                container(reference_panel::view(self.state, self.block_id))
                    .width(Length::Fill)
                    .into()
            }
            | Some(BlockPanelBarState::Probe) | None => column![].into(),
        }
    }
}
