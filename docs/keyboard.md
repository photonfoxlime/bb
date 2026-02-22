# Keyboard Navigation

Arrow-key navigation moves focus between blocks when the cursor is at the edge of a text editor.

For module structure and key types, see [architecture.md](architecture.md).

## Block Traversal (Up/Down)

- **Edge detection**: In the `PointEdited` handler, before performing an Up/Down action, the current cursor position is checked. If Up is pressed on line 0, or Down is pressed on the last line, the action is intercepted.
- **DFS ordering**: `BlockStore::next_visible_in_dfs` and `prev_visible_in_dfs` walk the tree in visual DFS order, skipping collapsed subtrees.
- **Focus transfer**: Each text editor has a stable `widget::Id` (stored in `EditorStore::widget_ids`). On traversal, `widget::operation::focus(target_id)` is returned as the `Task`, moving keyboard focus to the adjacent block.
- **Collapse awareness**: Collapsed blocks are passed as a `HashSet<BlockId>` to the DFS helpers, so folded subtrees are skipped during traversal.
