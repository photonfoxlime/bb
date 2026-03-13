//! Keyboard shortcut dispatch and shortcut-help banner metadata.
//!
//! This module keeps runtime shortcut routing and the banner inventory close to
//! each other so future refactors can converge on one source of truth without
//! scattering shortcut semantics across unrelated UI modules.

use super::*;
use crate::store::Direction;
use rust_i18n::t;

/// Keyboard shortcuts for block focus navigation and structural movement.
///
/// Keymap:
/// - macOS:
///   `Ctrl+Up/Down/Left/Right` / `Ctrl+Shift+Up/Down/Left/Right` /
///   `Cmd+[` / `Cmd+]`
/// - Other platforms:
///   `Alt+Up/Down/Left/Right` / `Alt+Shift+Up/Down/Left/Right` /
///   `Ctrl+[` / `Ctrl+]`
///
/// - `Up` / `Down`: focus previous/next sibling (wrap at boundaries).
/// - `Left`: focus parent.
/// - `Right`: focus first child (if any).
/// - `Shift+Up` / `Shift+Down`: move block among siblings (wrap).
/// - `Shift+Left`: outdent block to be after its parent.
/// - `Shift+Right`: indent block as first child of previous sibling.
///
/// These shortcuts are document-view operations and are ignored in settings
/// view and pick-friend mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementShortcut {
    FocusSiblingPrevious,
    FocusSiblingNext,
    FocusParent,
    FocusFirstChild,
    MoveSiblingPrevious,
    MoveSiblingNext,
    MoveAfterParent,
    MoveToPreviousSiblingFirstChild,
}

/// Messages for keyboard shortcut dispatch.
#[derive(Debug, Clone)]
pub enum ShortcutMessage {
    Trigger(ActionId),
    ForBlock { block_id: BlockId, action_id: ActionId },
    Movement(MovementShortcut),
}

/// Banner section grouping for the keyboard-shortcuts help surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShortcutSection {
    /// Global shortcuts that work outside one focused row action set.
    Global,
    /// Structure-changing shortcuts backed by [`ActionId`] values.
    Structure,
    /// Tree navigation and structural movement shortcuts.
    Movement,
    /// Backspace-driven mode transitions and destructive behavior.
    BackspaceBehavior,
    /// Click gestures available while multiselect mode is active.
    Multiselect,
}

impl ShortcutSection {
    /// Return the i18n key for this section title.
    fn title_key(self) -> &'static str {
        match self {
            | Self::Global => "shortcut_help_section_global",
            | Self::Structure => "shortcut_help_section_structure",
            | Self::Movement => "shortcut_help_section_movement",
            | Self::BackspaceBehavior => "shortcut_help_section_backspace",
            | Self::Multiselect => "shortcut_help_section_multiselect",
        }
    }
}

/// Stable identifier for one banner row in the keyboard-shortcuts help surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShortcutId {
    /// Toggle the global find overlay.
    GlobalToggleFind,
    /// Jump to the next global find result.
    GlobalFindNext,
    /// Jump to the previous global find result.
    GlobalFindPrevious,
    /// Undo the most recent document mutation.
    GlobalUndo,
    /// Redo the most recently undone document mutation.
    GlobalRedo,
    /// Close the active overlay, settings screen, or block panel.
    GlobalEscape,
    /// Structure shortcut backed by an existing action identifier.
    ///
    /// Note: structure rows reuse [`ActionId`] so later dispatch unification can
    /// point banner metadata at the same semantic action identity.
    Action(ActionId),
    /// Move the caret by word inside focused text editors.
    MovementWordCursor,
    /// Move focus to a related block without changing structure.
    MovementFocus,
    /// Reorder the focused block among its siblings.
    MovementReorder,
    /// Move the focused block after its parent.
    MovementOutdent,
    /// Move the focused block into the previous sibling as its first child.
    MovementIndent,
    /// Enter multiselect mode by backspacing on an empty point.
    BackspaceEnterMultiselect,
    /// Delete the current multiselect selection with backspace.
    BackspaceDeleteMultiselect,
    /// Select one block while multiselect mode is active.
    MultiselectClick,
    /// Select a contiguous multiselect range.
    MultiselectRangeSelect,
    /// Toggle one block in the multiselect set.
    MultiselectToggle,
}

/// Platform-aware display formatting for one shortcut chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShortcutChord {
    /// Render the provided label exactly as-is.
    Literal(&'static str),
    /// Render a shared `Cmd/Ctrl + …` label.
    ///
    /// Note: this is intentionally banner-oriented formatting. Runtime dispatch
    /// still owns the actual modifier parsing rules in this change.
    CommandOrCtrl(&'static str),
    /// Render one label on macOS and another elsewhere.
    Platform {
        /// Chord label shown on macOS builds.
        macos: &'static str,
        /// Chord label shown on non-macOS builds.
        other: &'static str,
    },
}

impl ShortcutChord {
    /// Format this chord for the current target platform.
    pub(crate) fn format(self) -> String {
        match self {
            | Self::Literal(label) => label.to_string(),
            | Self::CommandOrCtrl(key) => format!("Cmd/Ctrl + {key}"),
            | Self::Platform { macos, other } => {
                #[cfg(target_os = "macos")]
                {
                    let _ = other;
                    macos.to_string()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = macos;
                    other.to_string()
                }
            }
        }
    }
}

/// One shortcut row specification in the shared shortcut registry.
///
/// The spec keeps stable identity, grouping, and display metadata. Runtime
/// dispatch resolves through the same [`ShortcutId`] inventory, but key-matching
/// rules still live in dedicated helper functions below because several rows
/// intentionally share one display label while differing in event-path scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ShortcutSpec {
    /// Stable row identifier used by tests and future dispatch convergence.
    pub id: ShortcutId,
    /// Section that should contain this row.
    pub section: ShortcutSection,
    /// Chord formatting descriptor for the row's left column.
    pub chord: ShortcutChord,
    /// I18n key for the row description shown in the right column.
    pub description_key: &'static str,
}

/// Fully formatted banner row ready for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShortcutRowVm {
    /// Stable identity preserved from the source spec.
    pub id: ShortcutId,
    /// Rendered chord label for the left column.
    pub chord: String,
    /// Localized description for the right column.
    pub description: String,
}

/// Fully formatted banner section ready for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShortcutSectionVm {
    /// Stable section identity preserved from the source spec.
    pub section: ShortcutSection,
    /// Localized section title.
    pub title: String,
    /// Ordered rows for this section.
    pub rows: Vec<ShortcutRowVm>,
}

/// Namespace for the shared shortcut metadata registry.
///
/// The catalog drives both the shortcut-help banner and the runtime lookup
/// helpers in this module. Display metadata remains declarative in
/// [`ShortcutSpec`], while the matching helpers encode the remaining
/// event-source details that are not yet practical to express as static data.
///
/// Note: multiselect click gestures and the backspace behavior rows remain
/// display-only because they are described in the banner but dispatched through
/// specialized non-shortcut event paths.
pub(crate) struct ShortcutCatalog;

impl ShortcutCatalog {
    /// Ordered section list for the shortcut-help banner.
    pub(crate) fn banner_sections() -> &'static [ShortcutSection] {
        const SECTIONS: &[ShortcutSection] = &[
            ShortcutSection::Global,
            ShortcutSection::Structure,
            ShortcutSection::Movement,
            ShortcutSection::BackspaceBehavior,
            ShortcutSection::Multiselect,
        ];
        SECTIONS
    }

    /// Ordered row specifications for the shortcut-help banner.
    pub(crate) fn banner_specs() -> &'static [ShortcutSpec] {
        const SPECS: &[ShortcutSpec] = &[
            ShortcutSpec {
                id: ShortcutId::GlobalToggleFind,
                section: ShortcutSection::Global,
                chord: ShortcutChord::Platform { macos: "Cmd + F", other: "Ctrl + F" },
                description_key: "shortcut_help_desc_toggle_find",
            },
            ShortcutSpec {
                id: ShortcutId::GlobalFindNext,
                section: ShortcutSection::Global,
                chord: ShortcutChord::Platform { macos: "Cmd + G", other: "Ctrl + G" },
                description_key: "shortcut_help_desc_next_find_result",
            },
            ShortcutSpec {
                id: ShortcutId::GlobalFindPrevious,
                section: ShortcutSection::Global,
                chord: ShortcutChord::Platform {
                    macos: "Cmd + Shift + G",
                    other: "Ctrl + Shift + G",
                },
                description_key: "shortcut_help_desc_previous_find_result",
            },
            ShortcutSpec {
                id: ShortcutId::GlobalUndo,
                section: ShortcutSection::Global,
                chord: ShortcutChord::Platform { macos: "Cmd + Z", other: "Ctrl + Z" },
                description_key: "ctx_undo",
            },
            ShortcutSpec {
                id: ShortcutId::GlobalRedo,
                section: ShortcutSection::Global,
                chord: ShortcutChord::Platform {
                    macos: "Cmd + Shift + Z",
                    other: "Ctrl + Shift + Z",
                },
                description_key: "ctx_redo",
            },
            ShortcutSpec {
                id: ShortcutId::GlobalEscape,
                section: ShortcutSection::Global,
                chord: ShortcutChord::Literal("Escape"),
                description_key: "shortcut_help_desc_escape",
            },
            ShortcutSpec {
                id: ShortcutId::Action(ActionId::Amplify),
                section: ShortcutSection::Structure,
                chord: ShortcutChord::CommandOrCtrl("."),
                description_key: "action_amplify",
            },
            ShortcutSpec {
                id: ShortcutId::Action(ActionId::Distill),
                section: ShortcutSection::Structure,
                chord: ShortcutChord::CommandOrCtrl(","),
                description_key: "action_distill",
            },
            ShortcutSpec {
                id: ShortcutId::Action(ActionId::Atomize),
                section: ShortcutSection::Structure,
                chord: ShortcutChord::CommandOrCtrl("/"),
                description_key: "action_atomize",
            },
            ShortcutSpec {
                id: ShortcutId::Action(ActionId::AddChild),
                section: ShortcutSection::Structure,
                chord: ShortcutChord::CommandOrCtrl("Enter"),
                description_key: "action_add_child",
            },
            ShortcutSpec {
                id: ShortcutId::Action(ActionId::AddSibling),
                section: ShortcutSection::Structure,
                chord: ShortcutChord::CommandOrCtrl("Shift + Enter"),
                description_key: "action_add_sibling",
            },
            ShortcutSpec {
                id: ShortcutId::Action(ActionId::AcceptAll),
                section: ShortcutSection::Structure,
                chord: ShortcutChord::CommandOrCtrl("Shift + A"),
                description_key: "action_accept_all",
            },
            ShortcutSpec {
                id: ShortcutId::MovementWordCursor,
                section: ShortcutSection::Movement,
                chord: ShortcutChord::Platform {
                    macos: "Option + ←/→", other: "Ctrl + ←/→"
                },
                description_key: "shortcut_help_desc_move_cursor_by_word",
            },
            ShortcutSpec {
                id: ShortcutId::MovementFocus,
                section: ShortcutSection::Movement,
                chord: ShortcutChord::Platform {
                    macos: "Ctrl + ←/↑/→/↓", other: "Alt + ←/↑/→/↓"
                },
                description_key: "shortcut_help_desc_focus_relative_block",
            },
            ShortcutSpec {
                id: ShortcutId::MovementReorder,
                section: ShortcutSection::Movement,
                chord: ShortcutChord::Platform {
                    macos: "Ctrl + Shift + ↑/↓",
                    other: "Alt + Shift + ↑/↓",
                },
                description_key: "shortcut_help_desc_move_block_among_siblings",
            },
            ShortcutSpec {
                id: ShortcutId::MovementOutdent,
                section: ShortcutSection::Movement,
                chord: ShortcutChord::Platform {
                    macos: "Ctrl + Shift + ← / Cmd + [",
                    other: "Alt + Shift + ← / Ctrl + [",
                },
                description_key: "shortcut_help_desc_move_block_after_parent",
            },
            ShortcutSpec {
                id: ShortcutId::MovementIndent,
                section: ShortcutSection::Movement,
                chord: ShortcutChord::Platform {
                    macos: "Ctrl + Shift + → / Cmd + ]",
                    other: "Alt + Shift + → / Ctrl + ]",
                },
                description_key: "shortcut_help_desc_move_to_previous_sibling_first_child",
            },
            ShortcutSpec {
                id: ShortcutId::BackspaceEnterMultiselect,
                section: ShortcutSection::BackspaceBehavior,
                chord: ShortcutChord::Literal("Backspace"),
                description_key: "shortcut_help_desc_backspace_enter_multiselect",
            },
            ShortcutSpec {
                id: ShortcutId::BackspaceDeleteMultiselect,
                section: ShortcutSection::BackspaceBehavior,
                chord: ShortcutChord::Literal("Backspace"),
                description_key: "shortcut_help_desc_backspace_delete_multiselect",
            },
            ShortcutSpec {
                id: ShortcutId::MultiselectClick,
                section: ShortcutSection::Multiselect,
                chord: ShortcutChord::Literal("Click"),
                description_key: "shortcut_help_desc_multiselect_click",
            },
            ShortcutSpec {
                id: ShortcutId::MultiselectRangeSelect,
                section: ShortcutSection::Multiselect,
                chord: ShortcutChord::Literal("Shift + Click"),
                description_key: "shortcut_help_desc_multiselect_range_select",
            },
            ShortcutSpec {
                id: ShortcutId::MultiselectToggle,
                section: ShortcutSection::Multiselect,
                chord: ShortcutChord::CommandOrCtrl("Click"),
                description_key: "shortcut_help_desc_multiselect_toggle",
            },
        ];
        SPECS
    }

    /// Build localized banner view models for rendering.
    pub(crate) fn banner_view_model() -> Vec<ShortcutSectionVm> {
        Self::banner_sections()
            .iter()
            .copied()
            .map(|section| ShortcutSectionVm {
                section,
                title: t!(section.title_key()).to_string(),
                rows: Self::banner_specs()
                    .iter()
                    .filter(|spec| spec.section == section)
                    .map(|spec| ShortcutRowVm {
                        id: spec.id,
                        chord: spec.chord.format(),
                        description: t!(spec.description_key).to_string(),
                    })
                    .collect(),
            })
            .collect()
    }
}

impl ShortcutId {
    /// Return the structure action carried by this shortcut id, if any.
    fn action_id(self) -> Option<ActionId> {
        match self {
            | Self::Action(action_id) => Some(action_id),
            | _ => None,
        }
    }

    /// Convert one movement shortcut id into the runtime movement enum.
    fn movement_shortcut(self) -> Option<MovementShortcut> {
        match self {
            | Self::MovementFocus => None,
            | Self::MovementReorder => None,
            | Self::MovementWordCursor => None,
            | Self::MovementOutdent => Some(MovementShortcut::MoveAfterParent),
            | Self::MovementIndent => Some(MovementShortcut::MoveToPreviousSiblingFirstChild),
            | _ => None,
        }
    }

    /// Convert a movement row plus arrow direction into the runtime movement enum.
    fn movement_shortcut_for_arrow(self, key: &keyboard::Key) -> Option<MovementShortcut> {
        match (self, key) {
            | (Self::MovementFocus, keyboard::Key::Named(keyboard::key::Named::ArrowUp)) => {
                Some(MovementShortcut::FocusSiblingPrevious)
            }
            | (Self::MovementFocus, keyboard::Key::Named(keyboard::key::Named::ArrowDown)) => {
                Some(MovementShortcut::FocusSiblingNext)
            }
            | (Self::MovementFocus, keyboard::Key::Named(keyboard::key::Named::ArrowLeft)) => {
                Some(MovementShortcut::FocusParent)
            }
            | (Self::MovementFocus, keyboard::Key::Named(keyboard::key::Named::ArrowRight)) => {
                Some(MovementShortcut::FocusFirstChild)
            }
            | (Self::MovementReorder, keyboard::Key::Named(keyboard::key::Named::ArrowUp)) => {
                Some(MovementShortcut::MoveSiblingPrevious)
            }
            | (Self::MovementReorder, keyboard::Key::Named(keyboard::key::Named::ArrowDown)) => {
                Some(MovementShortcut::MoveSiblingNext)
            }
            | _ => self.movement_shortcut(),
        }
    }

    /// Convert one global shortcut id into the emitted application message.
    fn global_message(self) -> Option<Message> {
        match self {
            | Self::GlobalToggleFind => Some(Message::Find(FindMessage::Toggle)),
            | Self::GlobalFindNext => Some(Message::Find(FindMessage::JumpNext)),
            | Self::GlobalFindPrevious => Some(Message::Find(FindMessage::JumpPrevious)),
            | Self::GlobalUndo => Some(Message::UndoRedo(UndoRedoMessage::Undo)),
            | Self::GlobalRedo => Some(Message::UndoRedo(UndoRedoMessage::Redo)),
            | _ => None,
        }
    }
}

/// Resolve a structure action shortcut from a key press.
///
/// The action inventory comes from [`ShortcutCatalog`] so the help banner and
/// runtime dispatch stay aligned. The matching policy remains unchanged:
/// `Cmd` and `Ctrl` are both accepted to tolerate backend differences in editor
/// key events.
pub fn action_shortcut_from_key(
    key: keyboard::Key, modifiers: keyboard::Modifiers,
) -> Option<ActionId> {
    action_shortcut_id_from_key(&key, modifiers).and_then(ShortcutId::action_id)
}

/// Resolve a global non-row-specific shortcut from a key press.
///
/// Note: this intentionally excludes `Escape`, multiselect click gestures, and
/// the backspace behavior rows because those are handled by specialized event
/// paths rather than one shared shortcut dispatch entry point.
pub fn global_shortcut_message_from_key(
    key: &keyboard::Key, modifiers: keyboard::Modifiers,
) -> Option<Message> {
    global_shortcut_id_from_key(key, modifiers).and_then(ShortcutId::global_message)
}

fn action_shortcut_id_from_key(
    key: &keyboard::Key, modifiers: keyboard::Modifiers,
) -> Option<ShortcutId> {
    ShortcutCatalog::banner_specs().iter().find_map(|spec| match spec.id {
        | ShortcutId::Action(_) if action_shortcut_id_matches_key(spec.id, key, modifiers) => {
            Some(spec.id)
        }
        | _ => None,
    })
}

fn global_shortcut_id_from_key(
    key: &keyboard::Key, modifiers: keyboard::Modifiers,
) -> Option<ShortcutId> {
    ShortcutCatalog::banner_specs().iter().find_map(|spec| match spec.id {
        | ShortcutId::GlobalToggleFind
        | ShortcutId::GlobalFindNext
        | ShortcutId::GlobalFindPrevious
        | ShortcutId::GlobalUndo
        | ShortcutId::GlobalRedo
            if global_shortcut_id_matches_key(spec.id, key, modifiers) =>
        {
            Some(spec.id)
        }
        | _ => None,
    })
}

fn action_shortcut_id_matches_key(
    id: ShortcutId, key: &keyboard::Key, modifiers: keyboard::Modifiers,
) -> bool {
    match id {
        | ShortcutId::Action(ActionId::Amplify) => {
            matches_command_or_ctrl_character(key, modifiers, ".", false)
        }
        | ShortcutId::Action(ActionId::Distill) => {
            matches_command_or_ctrl_character(key, modifiers, ",", false)
        }
        | ShortcutId::Action(ActionId::Atomize) => {
            matches_command_or_ctrl_character(key, modifiers, "/", false)
        }
        | ShortcutId::Action(ActionId::AddChild) => {
            matches_command_or_ctrl_named(key, modifiers, keyboard::key::Named::Enter, false)
        }
        | ShortcutId::Action(ActionId::AddSibling) => {
            matches_command_or_ctrl_named(key, modifiers, keyboard::key::Named::Enter, true)
        }
        | ShortcutId::Action(ActionId::AcceptAll) => {
            matches_command_or_ctrl_character(key, modifiers, "a", true)
        }
        | ShortcutId::Action(_) | _ => false,
    }
}

fn global_shortcut_id_matches_key(
    id: ShortcutId, key: &keyboard::Key, modifiers: keyboard::Modifiers,
) -> bool {
    match id {
        | ShortcutId::GlobalToggleFind => matches_command_character(key, modifiers, "f", false),
        | ShortcutId::GlobalFindNext => matches_command_character(key, modifiers, "g", false),
        | ShortcutId::GlobalFindPrevious => matches_command_character(key, modifiers, "g", true),
        | ShortcutId::GlobalUndo => matches_command_character(key, modifiers, "z", false),
        | ShortcutId::GlobalRedo => matches_command_character(key, modifiers, "z", true),
        | _ => false,
    }
}

fn matches_command_or_ctrl_character(
    key: &keyboard::Key, modifiers: keyboard::Modifiers, value: &str, shifted: bool,
) -> bool {
    if !(modifiers.command() || modifiers.control()) {
        return false;
    }
    if modifiers.shift() != shifted {
        return false;
    }

    matches!(key, keyboard::Key::Character(candidate) if candidate.eq_ignore_ascii_case(value))
}

fn matches_command_or_ctrl_named(
    key: &keyboard::Key, modifiers: keyboard::Modifiers, named: keyboard::key::Named, shifted: bool,
) -> bool {
    if !(modifiers.command() || modifiers.control()) {
        return false;
    }
    if modifiers.shift() != shifted {
        return false;
    }

    matches!(key, keyboard::Key::Named(candidate) if *candidate == named)
}

fn matches_command_character(
    key: &keyboard::Key, modifiers: keyboard::Modifiers, value: &str, shifted: bool,
) -> bool {
    if !modifiers.command() {
        return false;
    }
    if modifiers.shift() != shifted {
        return false;
    }

    matches!(key, keyboard::Key::Character(candidate) if candidate.eq_ignore_ascii_case(value))
}

/// Direction for sibling traversal and reordering helpers.
///
/// Both directions use cyclic (wrap-around) semantics within one sibling
/// slice.
#[derive(Debug, Clone, Copy)]
enum SiblingDirection {
    Previous,
    Next,
}

/// Parse movement shortcuts from a key press.
///
/// Returns `None` when the key chord is not one of the declared movement
/// shortcuts or when extra command/control modifiers are pressed.
///
/// Design decision: this parser intentionally treats movement shortcuts as
/// global commands, independent of editor widget internals. The edit module
/// filters leaked editor actions so this parser remains the single source
/// of truth for movement dispatch.
pub fn movement_shortcut_from_key(
    key: &keyboard::Key, modified_key: &keyboard::Key, physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<ShortcutMessage> {
    if let Some(shortcut_id) =
        movement_shortcut_id_from_bracket_key(key, modified_key, physical_key, modifiers)
    {
        return shortcut_id.movement_shortcut_for_arrow(key).map(ShortcutMessage::Movement);
    }

    #[cfg(target_os = "macos")]
    if !modifiers.control() || modifiers.command() || modifiers.alt() {
        return None;
    }
    #[cfg(not(target_os = "macos"))]
    if !modifiers.alt() || modifiers.command() || modifiers.control() {
        return None;
    }

    let shortcut_id = match key {
        | keyboard::Key::Named(keyboard::key::Named::ArrowUp)
        | keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
            if modifiers.shift() {
                ShortcutId::MovementReorder
            } else {
                ShortcutId::MovementFocus
            }
        }
        | keyboard::Key::Named(keyboard::key::Named::ArrowLeft)
        | keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
            if modifiers.shift() {
                match key {
                    | keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                        ShortcutId::MovementOutdent
                    }
                    | keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                        ShortcutId::MovementIndent
                    }
                    | _ => unreachable!("left/right match arm already filtered the key"),
                }
            } else {
                ShortcutId::MovementFocus
            }
        }
        | _ => return None,
    };

    shortcut_id.movement_shortcut_for_arrow(key).map(ShortcutMessage::Movement)
}

/// Parse bracket aliases for structural movement shortcuts.
///
/// `[` maps to outdent and `]` maps to indent-into-previous-sibling.
///
/// Note: this stays as an alias layer over existing movement variants so
/// all execution paths (undo, tracing, and persistence) remain shared.
/// Note: `keyboard::Modifiers::command()` aliases Control on non-macOS, so
/// this parser must check the physical logo/command key explicitly.
fn movement_shortcut_id_from_bracket_key(
    key: &keyboard::Key, modified_key: &keyboard::Key, physical_key: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<ShortcutId> {
    #[cfg(target_os = "macos")]
    let has_bracket_modifier =
        modifiers.command() && !modifiers.control() && !modifiers.alt() && !modifiers.shift();
    #[cfg(not(target_os = "macos"))]
    let has_bracket_modifier =
        modifiers.control() && !modifiers.logo() && !modifiers.alt() && !modifiers.shift();

    if !has_bracket_modifier {
        return None;
    }

    movement_shortcut_id_from_bracket_logical_key(key)
        .or_else(|| movement_shortcut_id_from_bracket_logical_key(modified_key))
        .or_else(|| movement_shortcut_id_from_bracket_physical_key(physical_key))
}

fn movement_shortcut_id_from_bracket_logical_key(key: &keyboard::Key) -> Option<ShortcutId> {
    match key {
        | keyboard::Key::Character(value) if value == "[" => Some(ShortcutId::MovementOutdent),
        | keyboard::Key::Character(value) if value == "]" => Some(ShortcutId::MovementIndent),
        // Some backends can resolve command+bracket chords to browser-history
        // logical keys instead of the literal bracket character.
        | keyboard::Key::Named(keyboard::key::Named::BrowserBack)
        | keyboard::Key::Named(keyboard::key::Named::GoBack) => Some(ShortcutId::MovementOutdent),
        | keyboard::Key::Named(keyboard::key::Named::BrowserForward) => {
            Some(ShortcutId::MovementIndent)
        }
        | _ => None,
    }
}

fn movement_shortcut_id_from_bracket_physical_key(
    physical_key: keyboard::key::Physical,
) -> Option<ShortcutId> {
    match physical_key {
        | keyboard::key::Physical::Code(keyboard::key::Code::BracketLeft) => {
            Some(ShortcutId::MovementOutdent)
        }
        | keyboard::key::Physical::Code(keyboard::key::Code::BracketRight) => {
            Some(ShortcutId::MovementIndent)
        }
        | _ => None,
    }
}

pub fn handle(state: &mut AppState, message: ShortcutMessage) -> Task<Message> {
    match message {
        | ShortcutMessage::Trigger(action_id) => {
            let Some(block_id) = trigger_target_block_id(state) else {
                return Task::none();
            };
            run_shortcut_for_block(state, block_id, action_id)
        }
        | ShortcutMessage::ForBlock { block_id, action_id } => {
            // Don't change focus in PickFriend mode
            if state.ui().document_mode != DocumentMode::PickFriend {
                state.set_focus(block_id);
            }
            run_shortcut_for_block(state, block_id, action_id)
        }
        | ShortcutMessage::Movement(shortcut) => run_movement_shortcut(state, shortcut),
    }
}

/// Resolve the active block target for a global shortcut.
///
/// Priority:
/// 1. Explicit UI focus (`TransientUiState::focus`)
/// 2. Current edit session block (fallback for captured editor paths)
fn trigger_target_block_id(state: &AppState) -> Option<BlockId> {
    state.focus().map(|s| s.block_id).or(state.edit_session)
}

fn sibling_slice<'a>(state: &'a AppState, parent: Option<BlockId>) -> &'a [BlockId] {
    if let Some(parent_id) = parent {
        state.store.children(&parent_id)
    } else {
        state.store.roots()
    }
}

/// Resolve sibling focus target with cyclic wrap-around.
///
/// - Previous from index `0` wraps to the last sibling.
/// - Next from the last sibling wraps to index `0`.
fn sibling_wrap_target(
    state: &AppState, block_id: BlockId, direction: SiblingDirection,
) -> Option<BlockId> {
    let (parent, index) = state.store.parent_and_index_of(&block_id)?;
    let siblings = sibling_slice(state, parent);
    if siblings.is_empty() {
        return None;
    }

    let target_index = match direction {
        | SiblingDirection::Previous => {
            if index == 0 {
                siblings.len().saturating_sub(1)
            } else {
                index - 1
            }
        }
        | SiblingDirection::Next => {
            if index + 1 >= siblings.len() {
                0
            } else {
                index + 1
            }
        }
    };
    siblings.get(target_index).copied()
}

/// Focus a block and keep it visible in both fold and navigation scopes.
///
/// Order matters:
/// 1. unfold collapsed ancestors,
/// 2. reveal navigation path if needed,
/// 3. set focus and request widget focus,
/// 4. scroll the block into the visible viewport.
fn focus_block(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    unfold_folded_ancestors_for_focus(state, block_id);

    if !state.navigation.is_in_current_view(&state.store, &block_id) {
        state.navigation.reveal_parent_path(&state.store, &block_id);
    }
    state.set_focus(block_id);
    state.editor_buffers.ensure_block(&state.store, &block_id);
    let scroll = super::scroll::scroll_block_into_view(block_id);
    if let Some(widget_id) = state.editor_buffers.widget_id(&block_id) {
        return Task::batch([widget::operation::focus(widget_id.clone()), scroll]);
    }
    scroll
}

/// Ensure the focused target is visible by unfolding collapsed ancestors.
///
/// This is used by movement shortcuts that navigate or move blocks "into"
/// another block. If any ancestor on the target path is folded, it is
/// expanded before focus is applied.
fn unfold_folded_ancestors_for_focus(state: &mut AppState, block_id: BlockId) {
    let mut changed = false;
    let mut cursor = state.store.parent(&block_id);

    while let Some(parent_id) = cursor {
        if state.store.is_collapsed(&parent_id) {
            state.store.toggle_collapsed(&parent_id);
            tracing::info!(
                focused_block_id = ?block_id,
                unfolded_block_id = ?parent_id,
                "unfolded collapsed ancestor for movement shortcut"
            );
            changed = true;
        }
        cursor = state.store.parent(&parent_id);
    }

    if changed {
        state.persist_with_context("after unfolding folded ancestors for movement shortcut");
    }
}

fn focus_sibling(
    state: &mut AppState, block_id: BlockId, direction: SiblingDirection,
) -> Task<Message> {
    let Some(target_id) = sibling_wrap_target(state, block_id, direction) else {
        return Task::none();
    };
    tracing::debug!(from = ?block_id, to = ?target_id, ?direction, "focused sibling by shortcut");
    focus_block(state, target_id)
}

/// Move a block within its sibling list using cyclic semantics.
///
/// Boundary behavior mirrors focus navigation:
/// - Previous on first sibling moves to the end.
/// - Next on last sibling moves to the front.
fn move_block_within_siblings(
    state: &mut AppState, block_id: BlockId, direction: SiblingDirection,
) -> Task<Message> {
    let Some((parent, index)) = state.store.parent_and_index_of(&block_id) else {
        return Task::none();
    };
    let siblings = sibling_slice(state, parent).to_vec();
    if siblings.len() <= 1 {
        return Task::none();
    }

    let (target_id, move_dir) = match direction {
        | SiblingDirection::Previous => {
            if index == 0 {
                (siblings[siblings.len() - 1], Direction::After)
            } else {
                (siblings[index - 1], Direction::Before)
            }
        }
        | SiblingDirection::Next => {
            if index + 1 >= siblings.len() {
                (siblings[0], Direction::Before)
            } else {
                (siblings[index + 1], Direction::After)
            }
        }
    };

    state.mutate_with_undo_and_persist("after moving block within siblings by shortcut", |state| {
        if state.store.move_block(&block_id, &target_id, move_dir).is_some() {
            tracing::info!(block_id = ?block_id, target_id = ?target_id, ?move_dir, ?direction, "moved block within siblings by shortcut");
            true
        } else {
            false
        }
    });
    focus_block(state, block_id)
}

fn move_block_after_parent(state: &mut AppState, block_id: BlockId) -> Task<Message> {
    let Some(parent_id) = state.store.parent(&block_id) else {
        return Task::none();
    };

    state.mutate_with_undo_and_persist("after outdenting block by shortcut", |state| {
        if state.store.move_block(&block_id, &parent_id, Direction::After).is_some() {
            tracing::info!(block_id = ?block_id, parent_id = ?parent_id, "outdented block after parent by shortcut");
            true
        } else {
            false
        }
    });
    focus_block(state, block_id)
}

fn move_block_to_previous_sibling_first_child(
    state: &mut AppState, block_id: BlockId,
) -> Task<Message> {
    let Some((parent, index)) = state.store.parent_and_index_of(&block_id) else {
        return Task::none();
    };
    if index == 0 {
        return Task::none();
    }
    let siblings = sibling_slice(state, parent);
    let previous_sibling_id = siblings[index - 1];
    let first_child_of_previous = state.store.children(&previous_sibling_id).first().copied();

    let (target_id, move_dir) = if let Some(first_child_id) = first_child_of_previous {
        (first_child_id, Direction::Before)
    } else {
        (previous_sibling_id, Direction::Under)
    };

    state.mutate_with_undo_and_persist("after indenting block by shortcut", |state| {
        if state.store.move_block(&block_id, &target_id, move_dir).is_some() {
            tracing::info!(
                block_id = ?block_id,
                target_id = ?target_id,
                previous_sibling_id = ?previous_sibling_id,
                ?move_dir,
                "indented block into previous sibling by shortcut"
            );
            true
        } else {
            false
        }
    });
    focus_block(state, block_id)
}

fn run_movement_shortcut(state: &mut AppState, shortcut: MovementShortcut) -> Task<Message> {
    if state.ui().active_view != ViewMode::Document
        || state.ui().document_mode != DocumentMode::Normal
    {
        return Task::none();
    }

    let Some(block_id) = trigger_target_block_id(state) else {
        return Task::none();
    };

    match shortcut {
        | MovementShortcut::FocusSiblingPrevious => {
            focus_sibling(state, block_id, SiblingDirection::Previous)
        }
        | MovementShortcut::FocusSiblingNext => {
            focus_sibling(state, block_id, SiblingDirection::Next)
        }
        | MovementShortcut::FocusParent => {
            let Some(parent_id) = state.store.parent(&block_id) else {
                return Task::none();
            };
            tracing::debug!(from = ?block_id, to = ?parent_id, "focused parent by shortcut");
            focus_block(state, parent_id)
        }
        | MovementShortcut::FocusFirstChild => {
            let Some(child_id) = state.store.children(&block_id).first().copied() else {
                return Task::none();
            };
            tracing::debug!(from = ?block_id, to = ?child_id, "focused first child by shortcut");
            focus_block(state, child_id)
        }
        | MovementShortcut::MoveSiblingPrevious => {
            move_block_within_siblings(state, block_id, SiblingDirection::Previous)
        }
        | MovementShortcut::MoveSiblingNext => {
            move_block_within_siblings(state, block_id, SiblingDirection::Next)
        }
        | MovementShortcut::MoveAfterParent => move_block_after_parent(state, block_id),
        | MovementShortcut::MoveToPreviousSiblingFirstChild => {
            move_block_to_previous_sibling_first_child(state, block_id)
        }
    }
}

fn run_shortcut_for_block(
    state: &mut AppState, block_id: BlockId, action_id: ActionId,
) -> Task<Message> {
    let point_text =
        state.editor_buffers.get(&block_id).map(text_editor::Content::text).unwrap_or_default();
    let amplification_draft = state.store.amplification_draft(&block_id);
    let atomization_draft = state.store.atomization_draft(&block_id);
    let distillation_draft = state.store.distillation_draft(&block_id);
    let row_context = RowContext {
        block_id,
        point_text,
        has_draft: amplification_draft.is_some()
            || atomization_draft.is_some()
            || distillation_draft.is_some(),
        draft_suggestion_count: amplification_draft.map(|d| d.children.len()).unwrap_or(0)
            + atomization_draft.map(|d| d.points.len()).unwrap_or(0)
            + distillation_draft.map(|d| d.redundant_children.len()).unwrap_or(0),
        has_amplify_error: state.llm_requests.has_amplify_error(block_id),
        has_distill_error: state.llm_requests.has_distill_error(block_id),
        has_atomize_error: state.llm_requests.has_atomize_error(block_id),
        is_amplifying: state.llm_requests.is_amplifying(block_id),
        is_distilling: state.llm_requests.is_distilling(block_id),
        is_atomizing: state.llm_requests.is_atomizing(block_id),
        is_mounted: state.store.mount_table().entry(block_id).is_some(),
        has_children: !state.store.children(&block_id).is_empty(),
        is_unexpanded_mount: state.store.node(&block_id).is_some_and(|n| n.mount_path().is_some()),
    };
    let vm = project_for_viewport(build_action_bar_vm(&row_context), ViewportBucket::Wide);

    let is_enabled = vm
        .primary
        .iter()
        .chain(vm.contextual.iter())
        .chain(vm.overflow.iter())
        .find(|item| item.id == action_id)
        .is_some_and(|descriptor| descriptor.availability == ActionAvailability::Enabled);

    if is_enabled && let Some(next) = action_to_message_by_id(state, &block_id, action_id) {
        return AppState::update(state, next);
    }

    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn banner_section(section: ShortcutSection) -> ShortcutSectionVm {
        ShortcutCatalog::banner_view_model()
            .into_iter()
            .find(|candidate| candidate.section == section)
            .expect("banner section exists")
    }

    #[test]
    fn banner_specs_are_grouped_in_declared_section_order() {
        let expected_sections = ShortcutCatalog::banner_sections();
        let mut current_section_index = 0;

        for spec in ShortcutCatalog::banner_specs() {
            while expected_sections
                .get(current_section_index)
                .is_some_and(|section| *section != spec.section)
            {
                current_section_index += 1;
            }

            assert_eq!(expected_sections.get(current_section_index), Some(&spec.section));
        }
    }

    #[test]
    fn banner_specs_do_not_repeat_shortcut_ids() {
        let ids = ShortcutCatalog::banner_specs().iter().map(|spec| spec.id).collect::<Vec<_>>();
        let unique_ids = ids.iter().copied().collect::<HashSet<_>>();
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn command_or_ctrl_chords_format_with_shared_modifier_label() {
        assert_eq!(ShortcutChord::CommandOrCtrl("Enter").format(), "Cmd/Ctrl + Enter");
    }

    #[test]
    fn platform_chords_format_for_the_current_target() {
        let formatted = ShortcutChord::Platform { macos: "Cmd + F", other: "Ctrl + F" }.format();

        #[cfg(target_os = "macos")]
        assert_eq!(formatted, "Cmd + F");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(formatted, "Ctrl + F");
    }

    #[test]
    fn structure_section_preserves_current_action_order() {
        let row_ids = banner_section(ShortcutSection::Structure)
            .rows
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>();

        assert_eq!(
            row_ids,
            vec![
                ShortcutId::Action(ActionId::Amplify),
                ShortcutId::Action(ActionId::Distill),
                ShortcutId::Action(ActionId::Atomize),
                ShortcutId::Action(ActionId::AddChild),
                ShortcutId::Action(ActionId::AddSibling),
                ShortcutId::Action(ActionId::AcceptAll),
            ]
        );
    }

    #[test]
    fn movement_section_includes_the_current_five_rows() {
        let row_ids = banner_section(ShortcutSection::Movement)
            .rows
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>();

        assert_eq!(
            row_ids,
            vec![
                ShortcutId::MovementWordCursor,
                ShortcutId::MovementFocus,
                ShortcutId::MovementReorder,
                ShortcutId::MovementOutdent,
                ShortcutId::MovementIndent,
            ]
        );
    }

    fn movement_with_unidentified_physical(
        key: keyboard::Key, modifiers: keyboard::Modifiers,
    ) -> Option<ShortcutMessage> {
        movement_shortcut_from_key(
            &key,
            &key,
            keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            modifiers,
        )
    }

    fn movement_with_physical_bracket_code(
        key: keyboard::Key, modified_key: keyboard::Key, physical_code: keyboard::key::Code,
        modifiers: keyboard::Modifiers,
    ) -> Option<ShortcutMessage> {
        movement_shortcut_from_key(
            &key,
            &modified_key,
            keyboard::key::Physical::Code(physical_code),
            modifiers,
        )
    }

    fn bracket_modifiers_for_current_platform() -> keyboard::Modifiers {
        #[cfg(target_os = "macos")]
        {
            keyboard::Modifiers::COMMAND
        }
        #[cfg(not(target_os = "macos"))]
        {
            keyboard::Modifiers::CTRL
        }
    }

    #[test]
    fn global_shortcut_registry_resolves_find_toggle() {
        let message = global_shortcut_message_from_key(
            &keyboard::Key::Character("f".into()),
            keyboard::Modifiers::COMMAND,
        );
        assert!(matches!(message, Some(Message::Find(FindMessage::Toggle))));
    }

    #[test]
    fn global_shortcut_registry_resolves_redo() {
        let message = global_shortcut_message_from_key(
            &keyboard::Key::Character("z".into()),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
        );
        assert!(matches!(message, Some(Message::UndoRedo(UndoRedoMessage::Redo))));
    }

    #[test]
    fn action_shortcut_registry_resolves_accept_all() {
        let action = action_shortcut_from_key(
            keyboard::Key::Character("a".into()),
            keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
        );
        assert_eq!(action, Some(ActionId::AcceptAll));
    }

    #[test]
    fn trigger_uses_edit_session_when_focus_is_missing() {
        let (mut state, root) = AppState::test_state();
        assert!(state.focus().is_none());
        state.edit_session = Some(root);

        let _ = handle(&mut state, ShortcutMessage::Trigger(ActionId::Amplify));

        assert!(state.llm_requests.is_amplifying(root));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn alt_arrow_shortcuts_map_to_movement_commands() {
        let modifiers = keyboard::Modifiers::ALT;
        let up = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowUp),
            modifiers,
        );
        let left = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft),
            modifiers,
        );
        assert!(matches!(
            up,
            Some(ShortcutMessage::Movement(MovementShortcut::FocusSiblingPrevious))
        ));
        assert!(matches!(left, Some(ShortcutMessage::Movement(MovementShortcut::FocusParent))));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ctrl_arrow_shortcuts_map_to_movement_commands_on_macos() {
        let modifiers = keyboard::Modifiers::CTRL;
        let up = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowUp),
            modifiers,
        );
        let left = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft),
            modifiers,
        );
        assert!(matches!(
            up,
            Some(ShortcutMessage::Movement(MovementShortcut::FocusSiblingPrevious))
        ));
        assert!(matches!(left, Some(ShortcutMessage::Movement(MovementShortcut::FocusParent))));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn alt_shift_arrow_shortcuts_map_to_move_commands() {
        let modifiers = keyboard::Modifiers::ALT | keyboard::Modifiers::SHIFT;
        let down = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowDown),
            modifiers,
        );
        let right = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowRight),
            modifiers,
        );
        assert!(matches!(down, Some(ShortcutMessage::Movement(MovementShortcut::MoveSiblingNext))));
        assert!(matches!(
            right,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild))
        ));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ctrl_bracket_shortcuts_map_to_move_commands() {
        let modifiers = keyboard::Modifiers::CTRL;
        let left_bracket = movement_with_physical_bracket_code(
            keyboard::Key::Character("x".into()),
            keyboard::Key::Character("x".into()),
            keyboard::key::Code::BracketLeft,
            modifiers,
        );
        let right_bracket = movement_with_physical_bracket_code(
            keyboard::Key::Character("x".into()),
            keyboard::Key::Character("x".into()),
            keyboard::key::Code::BracketRight,
            modifiers,
        );

        assert!(matches!(
            left_bracket,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveAfterParent))
        ));
        assert!(matches!(
            right_bracket,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild))
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ctrl_shift_arrow_shortcuts_map_to_move_commands_on_macos() {
        let modifiers = keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT;
        let down = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowDown),
            modifiers,
        );
        let right = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::ArrowRight),
            modifiers,
        );
        assert!(matches!(down, Some(ShortcutMessage::Movement(MovementShortcut::MoveSiblingNext))));
        assert!(matches!(
            right,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild))
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn command_bracket_shortcuts_map_to_move_commands_on_macos() {
        let modifiers = keyboard::Modifiers::COMMAND;
        let left_bracket = movement_with_physical_bracket_code(
            keyboard::Key::Character("x".into()),
            keyboard::Key::Character("x".into()),
            keyboard::key::Code::BracketLeft,
            modifiers,
        );
        let right_bracket = movement_with_physical_bracket_code(
            keyboard::Key::Character("x".into()),
            keyboard::Key::Character("x".into()),
            keyboard::key::Code::BracketRight,
            modifiers,
        );

        assert!(matches!(
            left_bracket,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveAfterParent))
        ));
        assert!(matches!(
            right_bracket,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild))
        ));
    }

    #[test]
    fn bracket_shortcuts_accept_modified_key_and_browser_named_aliases() {
        let modifiers = bracket_modifiers_for_current_platform();
        let from_modified_left = movement_shortcut_from_key(
            &keyboard::Key::Character("x".into()),
            &keyboard::Key::Character("[".into()),
            keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            modifiers,
        );
        let from_modified_right = movement_shortcut_from_key(
            &keyboard::Key::Character("x".into()),
            &keyboard::Key::Character("]".into()),
            keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            modifiers,
        );
        let from_named_back = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::BrowserBack),
            modifiers,
        );
        let from_named_forward = movement_with_unidentified_physical(
            keyboard::Key::Named(keyboard::key::Named::BrowserForward),
            modifiers,
        );

        assert!(matches!(
            from_modified_left,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveAfterParent))
        ));
        assert!(matches!(
            from_modified_right,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild))
        ));
        assert!(matches!(
            from_named_back,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveAfterParent))
        ));
        assert!(matches!(
            from_named_forward,
            Some(ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild))
        ));
    }

    #[test]
    fn focus_sibling_previous_wraps_within_level() {
        let (mut state, root) = AppState::test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");
        state.set_focus(root);

        let _ =
            handle(&mut state, ShortcutMessage::Movement(MovementShortcut::FocusSiblingPrevious));

        assert_eq!(state.focus().map(|focus| focus.block_id), Some(sibling));
    }

    #[test]
    fn move_sibling_previous_wraps_within_level() {
        let (mut state, root) = AppState::test_state();
        let sibling = state
            .store
            .append_sibling(&root, "sibling".to_string())
            .expect("append sibling succeeds");
        state.set_focus(root);

        let _ =
            handle(&mut state, ShortcutMessage::Movement(MovementShortcut::MoveSiblingPrevious));

        assert_eq!(state.store.roots(), &[sibling, root]);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(root));
    }

    #[test]
    fn move_after_parent_outdents_block() {
        let (mut state, root) = AppState::test_state();
        let child =
            state.store.append_child(&root, "child".to_string()).expect("append child succeeds");
        state.set_focus(child);

        let _ = handle(&mut state, ShortcutMessage::Movement(MovementShortcut::MoveAfterParent));

        assert_eq!(state.store.parent(&child), None);
        assert_eq!(state.store.roots(), &[root, child]);
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(child));
    }

    #[test]
    fn move_to_previous_sibling_first_child_inserts_as_first_child() {
        let (mut state, root) = AppState::test_state();
        let first = state
            .store
            .append_child(&root, "first".to_string())
            .expect("append first child succeeds");
        let second = state
            .store
            .append_sibling(&first, "second".to_string())
            .expect("append second child succeeds");
        let existing = state
            .store
            .append_child(&first, "existing".to_string())
            .expect("append existing grandchild succeeds");
        state.set_focus(second);

        let _ = handle(
            &mut state,
            ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild),
        );

        assert_eq!(state.store.parent(&second), Some(first));
        let first_children = state.store.children(&first);
        assert_eq!(first_children.first().copied(), Some(second));
        assert!(first_children.contains(&existing));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(second));
    }

    #[test]
    fn focus_first_child_unfolds_current_block() {
        let (mut state, root) = AppState::test_state();
        let child =
            state.store.append_child(&root, "child".to_string()).expect("append child succeeds");
        state.store.toggle_collapsed(&root);
        state.set_focus(root);

        let _ = handle(&mut state, ShortcutMessage::Movement(MovementShortcut::FocusFirstChild));

        assert!(!state.store.is_collapsed(&root));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(child));
    }

    #[test]
    fn indent_into_previous_sibling_unfolds_target_parent() {
        let (mut state, root) = AppState::test_state();
        let first = state
            .store
            .append_child(&root, "first".to_string())
            .expect("append first child succeeds");
        let second = state
            .store
            .append_sibling(&first, "second".to_string())
            .expect("append second child succeeds");
        state.store.toggle_collapsed(&first);
        state.set_focus(second);

        let _ = handle(
            &mut state,
            ShortcutMessage::Movement(MovementShortcut::MoveToPreviousSiblingFirstChild),
        );

        assert!(!state.store.is_collapsed(&first));
        assert_eq!(state.store.parent(&second), Some(first));
        assert_eq!(state.focus().map(|focus| focus.block_id), Some(second));
    }
}
