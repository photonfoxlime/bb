use super::super::{AppState, ExpandState, Message, SummaryState};
use super::types::{ActionDescriptor, ActionId};
use crate::graph::BlockId;

pub fn action_to_message(
    state: &AppState, block_id: &BlockId, descriptor: &ActionDescriptor,
) -> Option<Message> {
    if !descriptor.availability.is_enabled() {
        return None;
    }

    action_to_message_by_id(state, block_id, descriptor.id)
}

pub fn action_to_message_by_id(
    state: &AppState, block_id: &BlockId, action_id: ActionId,
) -> Option<Message> {
    match action_id {
        | ActionId::Expand => Some(Message::Expand(block_id.clone())),
        | ActionId::Reduce => Some(Message::Summarize(block_id.clone())),
        | ActionId::AddChild => Some(Message::AddChild(block_id.clone())),
        | ActionId::AcceptAll => Some(Message::AcceptAllExpandedChildren(block_id.clone())),
        | ActionId::Retry => retry_message_for_block(state, block_id),
        | ActionId::DismissDraft => Some(Message::DiscardExpansion(block_id.clone())),
        | ActionId::AddSibling => Some(Message::AddSibling(block_id.clone())),
        | ActionId::DuplicateBlock => Some(Message::DuplicateBlock(block_id.clone())),
        | ActionId::ArchiveBlock => Some(Message::ArchiveBlock(block_id.clone())),
        | ActionId::Overflow
        | ActionId::CollapseBranch
        | ActionId::ExpandBranch
        | ActionId::OpenAsFocus => None,
    }
}

fn retry_message_for_block(state: &AppState, block_id: &BlockId) -> Option<Message> {
    if matches!(&state.expand_state, ExpandState::Error { block_id: id, .. } if id == block_id) {
        return Some(Message::Expand(block_id.clone()));
    }
    if matches!(&state.summary_state, SummaryState::Error { block_id: id, .. } if id == block_id) {
        return Some(Message::Summarize(block_id.clone()));
    }
    None
}
