# Expansion Draft Diff Rendering

For overall architecture and document index, see [architecture.md](architecture.md).

## Current Behavior

Expansion draft UI has two sections:

1. **Rewrite diff** (when `draft.rewrite` exists)
2. **Child suggestions list** (plain text, no diff)

Reduce drafts use the same rewrite-style diff renderer.

## Diff Algorithm

Implemented in `src/app/diff.rs`:

- tokenization preserves whitespace tokens,
- `similar::TextDiff` compares word-token slices,
- output is a sequence of `WordChange::{Unchanged, Deleted, Added}`.

Renderer in `src/app/view.rs` draws:

- old line with deletions highlighted,
- new line with additions highlighted.

## Styling

- Deletions: `theme::diff_deletion`
- Additions: `theme::diff_addition`
- Context: `theme::diff_context`

Diff appears inside the draft panel (`theme::draft_panel`) with existing apply/reject controls.

## Scope Decision

Children are intentionally not diffed:

- they are new suggestions, not edits to an existing child buffer,
- plain list + per-item keep/drop is easier to scan and decide quickly.
