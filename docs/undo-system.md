# Undo System

For overall architecture and document index, see [architecture.md](architecture.md).

## Overview

Undo/redo is graph-level and store-centric.

- `Cmd+Z`: undo
- `Cmd+Shift+Z`: redo
- New mutation clears redo future.

## Snapshot Model

`UndoSnapshot` stores only `BlockStore`.

Reasoning: drafts and mount metadata now live inside `BlockStore`, and editor widget state is reconstructed on restore.

`UndoHistory<T>` keeps fixed-capacity undo/redo stacks (default: 64).

## Mutation Boundary Policy

### General rule

Call `snapshot_for_undo()` before a meaningful state mutation.

### Coalesced text edits

Point edits are coalesced per active editing block:

- first edit on a block creates snapshot,
- subsequent edits on same block reuse that undo boundary,
- switching blocks or running structural actions resets coalescing.

This avoids one undo step per keystroke while preserving predictable block-level undo.

### Structured mutations

Most structural/draft mutations use `mutate_with_undo_and_persist(...)`, which enforces:

1. snapshot,
2. mutation closure,
3. persistence if mutation occurred.

## Restore Behavior

On undo/redo restore:

- `store` is replaced with snapshot store,
- `EditorStore` is rebuilt from store,
- in-flight reduce/expand tasks are aborted,
- pending request signatures and transient async states are cleared,
- restored state is persisted.

## Async Interaction Semantics

- Late async responses after undo/redo are ignored because pending signatures are cleared and lineage checks fail.
- Undo/redo is deterministic over committed snapshot state, not over future async completions.

## Covered Operations (high level)

- Structure: add/sibling/duplicate/archive/fold/mount transitions.
- Draft lifecycle: reduce/expand draft creation and apply/reject flows.
- Text edits: coalesced per block.

Transient UI state (`overflow_open_for`, focus markers, etc.) is not the source of truth for undo history.
