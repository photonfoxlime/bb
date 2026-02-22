# Undo System

For overall architecture and document index, see [architecture.md](architecture.md).

## Overview

Global undo/redo for graph-level operations on the block tree.
Cmd+Z undoes, Cmd+Shift+Z redoes (Ctrl on non-macOS). New mutations
discard the redo future.

## Scope

Undo covers structural and LLM-driven mutations to the `BlockStore`
and `expansion_drafts`:

| Operation | Undoable |
|-----------|----------|
| AddChild | Yes |
| AddSibling | Yes |
| DuplicateBlock | Yes |
| ArchiveBlock | Yes |
| AcceptExpandedChild | Yes |
| AcceptAllExpandedChildren | Yes |
| ApplyExpandedRewrite | Yes |
| SummarizeDone (success) | Yes |
| ExpandDone (success) | Yes -- undoing removes the expansion draft |
| Text editing (point edits) | Yes -- coalesced per block; switching blocks or performing a structural action starts a new undo entry |
| UI state (`overflow_open_for`, `active_block_id`, `focused_block_id`, `editing_block_id`) | No |

## Architecture

`UndoHistory` holds two `Vec<UndoSnapshot>` stacks (undo and redo) with a
configurable capacity (default 64). Each `UndoSnapshot` captures
`BlockStore` and `expansion_drafts`. The live state is never stored in
the stacks -- only prior states are.

### Snapshot protocol

Before any mutation, the handler calls `state.snapshot_for_undo()`,
which clones the current `BlockStore` and `expansion_drafts` into an
`UndoSnapshot` and pushes it onto the undo stack. This clears the redo
stack.

### Restore protocol

On undo, the current state is pushed to the redo stack and the top undo
entry becomes live. `EditorStore` is rebuilt from the restored graph,
expansion drafts are restored, and the file is persisted.

## Upgrading to a branching undo-tree

The public API (`push`, `undo`, `redo`) is stable. To support branching:

1. Replace the two `Vec` stacks with a tree where each node has a parent
   and a list of children.
2. Track a cursor pointing to the current node.
3. `undo` walks to the parent; `redo` walks to the most-recent child.
4. Add `undo_to_branch(n)` for explicit branch selection.

No changes needed outside `UndoHistory`.
