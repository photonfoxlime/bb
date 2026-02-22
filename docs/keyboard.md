# Keyboard Navigation

Arrow-key navigation moves focus between blocks when the cursor is at the edge of a text editor.

For module structure and key types, see [architecture.md](architecture.md).

## Block Traversal (Up/Down)

- **Edge detection**: In the `EditMessage::PointEdited` handler, Up/Down actions use a "try-and-compare" strategy. The cursor position is recorded before performing the action via cosmic_text. After the action, if the cursor position is unchanged, the editor is at a visual boundary (top or bottom, accounting for soft-wrapped lines). Only then does focus transfer to an adjacent block. This correctly handles wrapped lines within a single logical line.
- **DFS ordering**: `BlockStore::next_visible_in_dfs` and `prev_visible_in_dfs` walk the tree in visual DFS order, skipping collapsed subtrees.
- **Focus transfer**: Each text editor has a stable `widget::Id` (stored in `EditorStore::widget_ids`). On traversal, `widget::operation::focus(target_id)` is returned as the `Task`, moving keyboard focus to the adjacent block.
- **Collapse awareness**: Collapsed blocks are passed as a `HashSet<BlockId>` to the DFS helpers, so folded subtrees are skipped during traversal.
