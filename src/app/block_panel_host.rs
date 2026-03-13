//! Inline panel host for focused block-local panels.
//!
//! This module owns the chrome around block-local panels such as References and
//! Probe: the toggle bar that appears below the block row and the active
//! panel body shown beneath it.
//!
//! Keeping this host separate from `document.rs` narrows the future merge seam
//! for reference-style panels. `DocumentView` can treat the panel area as one
//! composable block instead of owning panel-specific button wiring.

use super::{
    AppState, Message, instruction_panel,
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

/// Focused block-local panel host rendered below a block row.
///
/// The host is intentionally state-less beyond borrowed app state and the
/// current row identity. It only projects persisted panel selection and emits
/// panel-toggle messages.
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

    /// Render the currently active panel body.
    ///
    /// Returns an empty element for non-focused rows or when no block-local
    /// panel is open.
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
            | Some(BlockPanelBarState::Probe) => {
                container(instruction_panel::view(self.state, self.block_id))
                    .width(Length::Fill)
                    .into()
            }
            | None => column![].into(),
        }
    }
}
