# Mount System

External file mounts for the block tree.

## Overview

A `BlockNode` is either inline children or a mount point referencing an
external JSON file. Mount nodes are loaded lazily: the file is read and
its blocks re-keyed into the main `SlotMap` only when the user expands
the node. All edits to mounted blocks are saved back to the originating
file immediately, alongside the main document save.

## BlockNode Enum

```rust
#[serde(untagged)]
enum BlockNode {
    Children { children: Vec<BlockId> },
    Mount { path: PathBuf },
}
```

`#[serde(untagged)]` means the JSON representation is transparent:
- `{"children": [...]}` for inline nodes (backward compatible).
- `{"path": "relative/sub.json"}` for mount points.

Mount paths are stored relative to the parent file when possible.
Resolution tries `base_dir.join(rel_path)` for relative paths; absolute
paths are used as-is.

## Lifecycle

### Expand

`BlockStore::expand_mount(mount_point, base_dir)`:

1. Read `BlockNode::Mount { path }` from the mount point. Error if the
   node is already `Children`.
2. Resolve path against `base_dir` (the directory containing the main
   blocks file, from `AppPaths::data_dir()`).
3. Canonicalize the resolved path and check the mount table for cycles.
4. Deserialize the file into a `BlockStore`.
5. Re-key every block from the sub-store into the main `SlotMap` with
   fresh `BlockId`s (`rekey_sub_store`). Mount nodes inside the sub-store
   are preserved as `BlockNode::Mount`, enabling recursive mounts.
6. Record origin (`BlockOrigin::Mounted`) for each re-keyed block.
7. Insert a `MountEntry` into the mount table with the canonical path,
   original relative path, root ids, and all block ids.
8. Swap the mount-point node to `BlockNode::Children` with the new roots.

### Collapse

`BlockStore::collapse_mount(mount_point)`:

1. Remove the `MountEntry` from the mount table (which also clears all
   origin records for the entry's blocks).
2. Remove all re-keyed blocks (nodes and points) from the main store.
3. Restore the mount-point node to `BlockNode::Mount { path }` using the
   entry's `rel_path` to preserve the original serialization form.

## Save Strategy

### Main document

`BlockStore::save()` calls `snapshot_for_save()` which:
- Clones the store.
- Restores every expanded mount-point back to `BlockNode::Mount { path }`
  using `entry.rel_path`.
- Removes all re-keyed blocks from the snapshot.

Result: the main file never contains mounted block data.

### Mounted files

`BlockStore::save_mounts()` iterates all mount entries and for each:
- Extracts the entry's blocks into a standalone `BlockStore` via
  `extract_mount_store`.
- Serializes and writes to `entry.path` (the canonical absolute path).

The app calls `save()` then `save_mounts()` in sequence via
`save_tree()` in `app.rs`.

## Cycle Detection

Before expanding, `expand_mount` canonicalizes the resolved path and
checks `MountTable::is_path_mounted(canonical)`. If any existing entry
already references the same canonical path, expansion returns
`MountError::CycleDetected { path }`.

This prevents:
- Direct self-reference (file A mounts file A).
- Indirect cycles (file A mounts file B which mounts file A, since
  both canonical paths would be in the table after the first expand).

## MountTable

Runtime-only (`#[serde(skip)]`), reconstructed by re-expanding mount
nodes after load.

```
MountTable
  origins: SecondaryMap<BlockId, BlockOrigin>   // per-block ownership
  entries: SecondaryMap<BlockId, MountEntry>     // per-mount-point metadata
```

Only blocks loaded from mounted files have an entry in `origins`.
`BlockOrigin::Mounted { mount_point }` records which mount loaded the block.
Blocks belonging to the main document are not tracked (absence = main).

`MountEntry` stores:
- `path` -- canonical absolute path for cycle detection and save-back.
- `rel_path` -- original relative path from `BlockNode::Mount` for
  serialization.
- `root_ids` -- re-keyed root block ids of the sub-store.
- `block_ids` -- all re-keyed block ids (roots + descendants).

## Undo Interaction

`MountTable` derives `Clone`. `UndoSnapshot` captures the full
`BlockStore` including the mount table. On undo/redo restore, the cloned
store replaces the current one, preserving mount state exactly as it was
at snapshot time. No special handling needed.

## UI Rendering

`render_block()` in `view.rs` dispatches on mount state:

- **Unexpanded mount** (`BlockNode::Mount`): renders the path label and
  a "Load" button that sends `Message::ExpandMount(id)`.
- **Expanded mount** (has a `MountTable` entry): renders the normal
  block editor plus a "Collapse" button prepended to the action row,
  sending `Message::CollapseMount(id)`.
- **Regular block**: unchanged rendering path.

`EditorStore` skips mount nodes naturally: `BlockStore::point()` returns
`None` for mount nodes (no point entry), so `populate()` continues past
them.

## Error Handling

`MountError` (in `src/mount.rs`, via `thiserror`):

| Variant | Cause |
|---------|-------|
| `NotAMount` | Tried to expand a `Children` node |
| `UnknownBlock` | Block id not in the store |
| `CycleDetected { path }` | Canonical path already mounted |
| `Read { path, source }` | File I/O failure |
| `Parse { path, source }` | JSON deserialization failure |

Surfaced to the UI through `AppError::Mount(UiError)` in `state.rs`.

## File Layout

```
src/mount.rs       -- MountTable, MountEntry, BlockOrigin, MountError
src/store.rs       -- BlockNode enum, expand/collapse/save logic
src/app.rs         -- ExpandMount/CollapseMount message handlers, save_tree()
src/app/view.rs    -- mount-aware block rendering
src/app/state.rs   -- AppError::Mount variant
src/paths.rs       -- AppPaths::data_dir() for base directory resolution
```
