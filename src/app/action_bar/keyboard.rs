use super::types::ActionId;
use iced::keyboard::{Key, Modifiers, key::Named};

/// Map a key press to an action shortcut, if any.
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
