# Action Bar

Design document for the action bar component.

For overall architecture and document index, see [architecture.md](architecture.md).

## Structure

Row zones: spine, marker, text editor, action buttons.

Status chip appears below the row (left-indented to align with text) when active (loading, error, or draft state). Hidden when idle.

- **Primary** (always visible): Expand, Reduce, Add child.
- **Contextual** (state-driven): Accept all, Retry, Cancel, Dismiss draft.
- **Overflow** (toggle menu): Add sibling, Duplicate, Archive, Collapse/Expand branch, Open as focus.

## Responsive Projection

| Bucket | Behavior |
|--------|----------|
| Wide | Primary + contextual inline |
| Medium | Contextual moves to overflow |
| Compact | Reduce also moves to overflow |
| TouchCompact | Menu-first access |

## Keyboard Shortcuts

`Ctrl+.` expand, `Ctrl+,` reduce, `Ctrl+Enter` add child, `Ctrl+Shift+Enter` add sibling, `Ctrl+Shift+A` accept all, `Ctrl+Backspace` archive. Pointer and keyboard both route through `ActionId -> dispatch`.

When a block point editor is focused, these shortcuts are handled by the editor widget's
`key_binding` hook and emitted as app messages (`Message::ShortcutFor`). This keeps
shortcuts available while typing, and still preserves normal text editing bindings
because unmatched keys fall back to iced's default editor bindings.

Shortcut target resolution in `AppState` is: `focused_block_id` first, then
`active_block_id`, then the first root block.

## State Rules

- Busy states disable only conflicting actions and expose `Cancel` for the active request.
- Error shows chip + retry. Draft surfaces accept/dismiss.
- Empty point gates reduce; add child remains available.
- Overflow auto-closes on outside click or Escape.

## Visual Implementation

- Action buttons: Lucide icons (size 16), annotation-style. Tooltips on all icons.
- Destructive buttons: danger color on hover/press.
- Status chip: below the row, shrink-width, Inter size 12, left-padded 16px. Only rendered when status is active.
- Expansion draft panel: tint background, spine-colored border, text buttons for rewrite/child accept/reject.

## Implementation

Module: `src/app/action_bar/` -- Typed action bar: types, selector (state-to-VM), responsive projection, keyboard shortcuts, dispatch.

Key types:

| Type | Location | Purpose |
|------|----------|---------|
| `ActionBarVm` | `action_bar/types.rs` | View model: primary, contextual, overflow action lists + status chip |
| `ActionId` | `action_bar/types.rs` | Enum of all action identifiers |
| `RowContext` | `action_bar/types.rs` | Inputs for building action bar VM per block row |
