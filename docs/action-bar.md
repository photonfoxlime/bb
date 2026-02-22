# Action Bar

For overall architecture and document index, see [architecture.md](architecture.md).

## Structure

Each block row has four zones: spine, marker, point editor, and action buttons.

- **Primary**: `Expand`, `Reduce`, `Add child`
- **Contextual**: `Accept all`, `Retry`, `Cancel`, `Dismiss`
- **Overflow**: `Add sibling`, `Duplicate`, `Save to file`, `Load from file`, `Archive`

Status chip appears below the row when loading, error, or draft state is active.

## Interaction Rules

- Busy states disable conflicting actions and expose `Cancel`.
- Empty point disables `Reduce`.
- Overflow closes on outside click and `Escape`.
- Mount-specific actions are hidden when not applicable (`is_mounted`, `is_unexpanded_mount`, `has_children`).

## Keyboard

- `Ctrl+.` expand
- `Ctrl+,` reduce
- `Ctrl+Enter` add child
- `Ctrl+Shift+Enter` add sibling
- `Ctrl+Shift+A` accept all
- `Ctrl+Backspace` archive

When an editor is focused, shortcuts are emitted through
`Message::Shortcut(ShortcutMessage::ForBlock { ... })`, so typing keeps default editor keybindings for unmatched keys.

Shortcut target fallback order is: focused block -> active block -> first root.

## Responsive Projection

`project_for_viewport` supports `Wide`, `Medium`, `Compact`, `TouchCompact`.
Today, production rendering uses `Wide`; other buckets are covered by tests and ready for runtime width wiring.

## Implementation Notes

- Module: `src/app/action_bar.rs`
- Core types: `ActionBarVm`, `ActionDescriptor`, `ActionId`, `RowContext`, `ViewportBucket`
- Mapping from action to app message lives in `action_to_message_by_id`.
