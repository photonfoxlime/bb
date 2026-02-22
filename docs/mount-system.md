# Mount System

For overall architecture and document index, see [architecture.md](architecture.md).

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
3. Canonicalize the resolved path for stable save-back behavior.
4. Deserialize the file into a `BlockStore`.
5. Re-key every block from the sub-store into the main `SlotMap` with
   fresh `BlockId`s (`rekey_sub_store`). Mount nodes inside the sub-store
   are preserved as `BlockNode::Mount`, enabling recursive mounts.
6. Record origin (`BlockOrigin::Mounted`) for each re-keyed block.
7. Insert a `MountEntry` into the mount table with the canonical path,
   original relative path, root ids, and all block ids.
8. Swap the mount-point node to `BlockNode::Children` with the new roots.

### Design decision: no cycle detection

Mount expansion intentionally does not perform cycle detection (direct or
indirect). Because mounts are loaded lazily and only when the user expands
them, recursive references are demand-driven instead of eagerly traversed.

Implications:
- Re-expanding the same file path at different mount points is allowed.
- Self-referential mount chains can exist and expand one step at a time.
- Safety remains bounded by explicit user actions (no automatic full-tree
  recursive expansion).

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
- Re-maps persisted draft keys (`expansion_drafts`, `reduction_drafts`) to the
  compacted key space and drops drafts for excluded mounted blocks.

Result: the main file never contains mounted block data.

### Mounted files

`BlockStore::save_mounts()` iterates all mount entries and for each:
- Extracts the entry's blocks into a standalone `BlockStore` via
  `extract_mount_store`.
- Carries over draft records for extracted blocks so mounted files preserve
  their own pending reduce/expand drafts.
- Serializes and writes to `entry.path` (the canonical absolute path).

The app calls `save()` then `save_mounts()` in sequence via
`save_tree()` in `app.rs`.

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
- `path` -- canonical absolute path for save-back.
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

`render_block()` in `view.rs` always renders the text editor and action
bar for every block, including mount nodes. Mounts only affect the
children portion of the node:

- **Unexpanded mount** (`BlockNode::Mount`): renders the standard
  text editor + action bar. The block marker is a right-pointing
  chevron (▸) instead of the usual dot. Clicking the chevron
  dispatches `Message::ExpandMount`. The action bar hides SaveToFile
  and LoadFromFile for unexpanded mounts (via `is_unexpanded_mount`
  in `RowContext`).
- **Expanded mount** (has a `MountTable` entry): renders the normal
  block editor with a down-pointing chevron (▾) as its marker.
  Clicking the chevron dispatches `Message::CollapseMount(id)`.
  Children are rendered normally below.
- **Regular block with children**: marker becomes a disclosure
  chevron (▾ expanded, ▸ collapsed). Clicking dispatches
  `Message::ToggleFold(id)`, toggling membership in the
  `AppState::collapsed` set. Collapsed blocks hide their children.
- **Leaf block**: unchanged dot marker.

This design unifies fold/unfold and mount load/unload behind a
single disclosure chevron gesture. The chevron replaces the dot
marker only when a block is foldable (has children, is an expanded
mount, or is an unexpanded mount). The node's own text and action
bar are always accessible regardless of mount or fold state.

`EditorStore` creates editor buffers for mount nodes because their
point text is preserved in the `points` SecondaryMap (set by
`set_mount_path`). The `populate()` method finds the point and
creates a buffer as usual.

## Error Handling

`MountError` (in `src/mount.rs`, via `thiserror`):

| Variant | Cause |
|---------|-------|
| `NotAMount` | Tried to expand a `Children` node |
| `UnknownBlock` | Block id not in the store |
| `Read { path, source }` | File I/O failure |
| `Parse { path, source }` | JSON deserialization failure |

Surfaced to the UI through `AppError::Mount(UiError)` in `state.rs`.

## Save to File

The inverse of expand: extracts a block's children into an external
file and replaces the block node with `BlockNode::Mount { path }`.
Accessible from the per-block action bar ("Save to file" in overflow).

### Visibility

The button is hidden when the block already has its children saved to
a file (i.e. `mount_table.entry(block_id)` returns `Some`), or when the
block is an unexpanded mount (`is_unexpanded_mount`). Blocks loaded
*from* a mounted file are still eligible -- only the mount point itself
is excluded. This enables recursive mounts: a subnode within a mounted
subtree can save its own children to a separate file.

### Flow

1. User clicks "Save to file". `Message::SaveToFile(block_id)` fires.
2. An `rfd::AsyncFileDialog` save dialog opens (`.json` filter).
3. On file selection, `Message::SaveToFilePicked(block_id, Some(path))`
   triggers the core logic.

### Core: `save_subtree_to_file`

`BlockStore::save_subtree_to_file(&mut self, block_id, path, base_dir)`:

1. Read the block's children list (the subtree roots).
2. Walk descendants with `collect_own_subtree_ids`, which:
   - Adds each visited block to `own_ids`.
   - Stops at expanded mount points (blocks with a `MountEntry`),
     recording them in `mount_points` instead of recursing into
     their mounted children.
3. Build a standalone `BlockStore`:
   - Allocate fresh `BlockId`s in a new `SlotMap` (re-keying).
   - Copy points from the main store.
   - Rewrite `Children` nodes with re-mapped ids.
   - For expanded mount points, write `BlockNode::Mount { path: entry.rel_path }`
     so the sub-store references the nested file rather than inlining
     the mounted content.
   - For unexpanded `Mount` nodes, copy verbatim.
   - Sub-store roots = re-mapped children of `block_id` (not `block_id`
     itself), ensuring `expand_mount` round-trips correctly.
4. Serialize and write to the chosen path.
5. Clean up the main store:
   - Remove mount entries and all blocks for nested expanded mounts.
   - Remove the subtree's own nodes, points, and origin records.
6. Compute a relative path via `path.strip_prefix(base_dir)`.
7. Replace the block's node with `BlockNode::Mount { path: rel_path }`.

### Post-save

After `save_subtree_to_file` succeeds, `app.rs`:

1. Immediately calls `expand_mount` on the same block so the user
   sees no disruption -- the children reappear as mounted content.
2. Calls `save_tree()` to persist both the updated main document
   (which now has a `Mount` node) and any mount files.

### Recursive mount handling

Mounts can nest arbitrarily. When saving a subtree that contains
expanded mount points:

- The expanded mount's own children (loaded from another file) are
  NOT included in the new sub-store. Instead, the mount point is
  written as `BlockNode::Mount { path }` in the sub-store.
- The expanded mount's entry and blocks are removed from the main
  store during cleanup, since they will be re-loaded when the
  parent mount is next expanded.

## Load from File

The inverse of save-to-file: converts a childless block into a mount
node pointing at a user-chosen JSON file, then immediately expands it.
Accessible from the per-block action bar ("Load from file" in overflow).

### Visibility

The button appears only when all conditions hold:
 The block has no children (`has_children == false`).
 The block is not already an expanded mount point (`is_mounted == false`).
 The block is not an unexpanded mount (`is_unexpanded_mount == false`).

Blocks with existing children are excluded because loading a file would
overwrite the block's content. Already-mounted blocks have their own
collapse/expand controls.

### Flow

1. User clicks "Load from file". `Message::LoadFromFile(block_id)` fires.
2. An `rfd::AsyncFileDialog` open dialog opens (`.json` filter).
3. On file selection, `Message::LoadFromFilePicked(block_id, Some(path))`
   triggers the core logic.

### Core: `set_mount_path`

`BlockStore::set_mount_path(&mut self, id, path) -> Option<()>`:

1. Verify the block exists and has no children. Return `None` otherwise.
2. Replace the block's node with `BlockNode::Mount { path }`.

After `set_mount_path`, the app calls `expand_mount` on the same block
to load the file contents into the tree, then ensures editors are
populated and saves the tree.

### Post-load

After `set_mount_path` + `expand_mount` succeed, `app.rs`:

1. Calls `ensure_editors()` so the newly loaded blocks get editor state.
2. Calls `save_tree()` to persist the updated main document (which now
   has a `Mount` node) and any mount files.

The user sees the childless block transform into an expanded mount with
the file's content displayed inline, identical to manually placing a
mount node and expanding it.

## File Layout

```
src/mount.rs           -- MountTable, MountEntry, BlockOrigin, MountError
src/store.rs           -- BlockNode enum, expand/collapse/save/save_subtree logic
src/app.rs             -- ExpandMount/CollapseMount/SaveToFile message handlers, save_tree()
src/app/action_bar.rs  -- SaveToFile/LoadFromFile action descriptors, is_mounted/has_children fields in RowContext
src/app/view.rs        -- mount-aware block rendering, is_mounted/has_children computation
src/app/state.rs       -- AppError::Mount variant
src/paths.rs           -- AppPaths::data_dir() for base directory resolution
```
