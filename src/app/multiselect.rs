//! Multiselect mode: block selection for batch actions.
//!
//! Handles click interactions (plain, Shift+range, Cmd+toggle) and
//! coordinates with [`DocumentMode::Multiselect`].
//!
//! ## Interaction semantics
//!
//! - **Plain click**: Replace selection with clicked block only.
//! - **Cmd/Ctrl+click**: Toggle block (add if absent, remove if present).
//! - **Shift+click**: Range select from anchor to clicked block (DFS order).

use super::AppState;
use crate::store::BlockId;

/// Handle a block click in multiselect mode.
///
/// Modifier semantics:
/// - **Plain click**: Replace selection with this block only. Anchor = clicked block.
/// - **Cmd/Ctrl+click**: Toggle this block (add if absent, remove if present). Anchor = clicked.
/// - **Shift+click**: Range select from anchor to this block (inclusive, DFS order). Anchor = clicked.
pub fn handle_block_clicked(state: &mut AppState, block_id: BlockId) {
    let modifiers = state.ui().keyboard_modifiers;

    if modifiers.command() || modifiers.control() {
        if state.ui().multiselect_selected_blocks.contains(&block_id) {
            state.ui_mut().multiselect_selected_blocks.remove(&block_id);
        } else {
            state.ui_mut().multiselect_selected_blocks.insert(block_id);
        }
        state.ui_mut().multiselect_anchor = Some(block_id);
        return;
    }

    if modifiers.shift() {
        let anchor = state.ui().multiselect_anchor.or(state
            .ui()
            .multiselect_selected_blocks
            .iter()
            .next()
            .copied());

        if let Some(from) = anchor {
            let range = visible_blocks_between(state, from, block_id);
            state.ui_mut().multiselect_selected_blocks.clear();
            for id in &range {
                state.ui_mut().multiselect_selected_blocks.insert(*id);
            }
        } else {
            state.ui_mut().multiselect_selected_blocks.insert(block_id);
        }
        state.ui_mut().multiselect_anchor = Some(block_id);
        return;
    }

    // Plain click: replace selection
    state.ui_mut().multiselect_selected_blocks.clear();
    state.ui_mut().multiselect_selected_blocks.insert(block_id);
    state.ui_mut().multiselect_anchor = Some(block_id);
}

/// Collect visible blocks between `from` and `to` in DFS order (inclusive).
///
/// Only includes blocks that are visible (expanded ancestors) and within
/// the current navigation view.
fn visible_blocks_between(state: &AppState, from: BlockId, to: BlockId) -> Vec<BlockId> {
    if from == to {
        if state.store.is_visible(&from) && state.navigation.is_in_current_view(&state.store, &from)
        {
            return vec![from];
        }
        return vec![];
    }

    let store = &state.store;
    let nav = &state.navigation;

    let mut out = Vec::new();
    let mut cur = from;

    loop {
        if !store.is_visible(&cur) || !nav.is_in_current_view(store, &cur) {
            if let Some(next) = store.next_visible_in_dfs(&cur) {
                cur = next;
                continue;
            }
            break;
        }
        out.push(cur);
        if cur == to {
            return out;
        }
        let Some(next) = store.next_visible_in_dfs(&cur) else {
            break;
        };
        cur = next;
    }

    // from is after to in DFS; walk backward from to
    out.clear();
    let mut cur = to;
    loop {
        if !store.is_visible(&cur) || !nav.is_in_current_view(store, &cur) {
            if let Some(prev) = store.prev_visible_in_dfs(&cur) {
                cur = prev;
                continue;
            }
            break;
        }
        out.push(cur);
        if cur == from {
            out.reverse();
            return out;
        }
        let Some(prev) = store.prev_visible_in_dfs(&cur) else {
            break;
        };
        cur = prev;
    }

    vec![]
}
