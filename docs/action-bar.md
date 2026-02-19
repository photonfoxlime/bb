# Action Bar

Design document for the action bar component. Separated from `design-understanding.md` for focused reference.

For overall architecture and aesthetic direction, see `design-understanding.md`.

## Structure

Row zones: spine, marker, text editor, status chip, action buttons.

- **Primary** (always visible): Expand, Reduce, Add child.
- **Contextual** (state-driven): Accept all, Retry, Dismiss draft.
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

## State Rules

- Busy states disable only conflicting actions.
- Error shows chip + retry. Draft surfaces accept/dismiss.
- Empty point gates reduce; add child remains available.
- Overflow auto-closes on outside click or Escape.

## Visual Implementation

- Action buttons: Lucide icons (size 16), annotation-style. Tooltips on all icons.
- Destructive buttons: danger color on hover/press.
- Status chip: shrink-width, Inter size 12.
- Expansion draft panel: tint background, spine-colored border, text buttons for rewrite/child accept/reject.

## Implementation

Module: `src/app/action_bar/` -- Typed action bar: types, selector (state-to-VM), responsive projection, keyboard shortcuts, dispatch.

Key types:

| Type | Location | Purpose |
|------|----------|---------|
| `ActionBarVm` | `action_bar/types.rs` | View model: primary, contextual, overflow action lists + status chip |
| `ActionId` | `action_bar/types.rs` | Enum of all action identifiers |
| `RowContext` | `action_bar/types.rs` | Inputs for building action bar VM per block row |
