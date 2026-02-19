use super::super::BlockId;
use super::types::{ActionBarVm, ActionId};
use iced::keyboard::{Key, Modifiers, key::Named};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionFocusState {
    pub focused_row: Option<BlockId>,
    pub focused_action_index: usize,
}

impl Default for ActionFocusState {
    fn default() -> Self {
        Self { focused_row: None, focused_action_index: 0 }
    }
}

pub fn visible_action_sequence(vm: &ActionBarVm) -> Vec<ActionId> {
    vm.visible_actions().into_iter().map(|action| action.id).collect::<Vec<_>>()
}

pub fn next_focus_index(current: usize, len: usize, reverse: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if reverse { if current == 0 { len - 1 } else { current - 1 } } else { (current + 1) % len }
}

pub fn shortcut_to_action(key: Key, modifiers: Modifiers) -> Option<ActionId> {
    if !modifiers.control() {
        return None;
    }

    if modifiers.shift() {
        match key {
            | Key::Named(Named::Enter) => return Some(ActionId::AddSibling),
            | Key::Character(value) if value.eq_ignore_ascii_case("a") => {
                return Some(ActionId::AcceptAll);
            }
            | _ => {}
        }
    }

    match key {
        | Key::Character(value) if value == "." => Some(ActionId::Expand),
        | Key::Character(value) if value == "," => Some(ActionId::Reduce),
        | Key::Named(Named::Enter) => Some(ActionId::AddChild),
        | Key::Named(Named::Backspace) => Some(ActionId::ArchiveBlock),
        | _ => None,
    }
}
