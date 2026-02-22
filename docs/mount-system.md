# Mount System

For overall architecture and document index, see [architecture.md](architecture.md).

## Purpose

Mounts let one block subtree live in a separate JSON file while remaining editable in place.

## Data Model

`BlockNode` is either:

- `Children { children: Vec<BlockId> }`
- `Mount { path: PathBuf }`

`MountTable` is runtime-only (`#[serde(skip)]`) and tracks:

- `entries`: mount-point metadata (`MountEntry`)
- `origins`: per-block ownership (`BlockOrigin::Mounted { mount_point }`)

## Core Invariants

1. Main-file snapshots never inline mounted descendants.
2. Expanded nested mounts are saved as mount links (`Mount { path }`), not expanded payloads.
3. Relative mount paths resolve against the owning file directory:
   - top-level mounts -> app data dir
   - nested mounts -> parent mount file dir
4. Collapsing a mount restores the mount-point node with `entry.rel_path`.

These invariants prevent path drift and duplicated ownership across files.

## Lifecycle

### Expand (`BlockStore::expand_mount`)

1. Read `Mount { path }` from mount point.
2. Resolve effective base dir (top-level vs nested).
3. Canonicalize path for stable save-back.
4. Load sub-store from file.
5. Re-key sub-store ids into main store.
6. Record ownership in `MountTable`.
7. Replace mount-point node with `Children` roots.

### Collapse (`BlockStore::collapse_mount`)

1. Remove mount entry.
2. Remove all loaded mounted blocks.
3. Restore mount-point node to `Mount { rel_path }`.

## Save Behavior

Detailed write ordering is in [persistence.md](persistence.md). Mount-specific behavior:

- Main snapshot strips mounted descendants and keeps mount links.
- `save_mounts()` extracts each expanded mount from live subtree state.
- Nested expanded mounts are collapsed to links during mounted-file serialization.

## UI Message Flow

- Expand unexpanded mount:
  `Message::MountFile(MountFileMessage::ExpandMount(block_id))`
- Collapse expanded mount:
  `Message::MountFile(MountFileMessage::CollapseMount(block_id))`
- Save subtree to file:
  `Message::MountFile(MountFileMessage::SaveToFilePicked { ... })`
- Load file into childless block:
  `Message::MountFile(MountFileMessage::LoadFromFilePicked { ... })`

Fold/unfold of regular blocks uses `StructureMessage::ToggleFold` and is separate from mount ownership.

## Design Tradeoff: No Cycle Detection

Mount expansion intentionally does not reject recursive path references.

- Why: mounts are lazy, user-driven operations; recursion does not auto-expand globally.
- Benefit: simple model and explicit user control.
- Cost: users can create recursive chains that only reveal themselves when repeatedly expanded.

The system remains bounded by explicit expand actions.

## Error Surface

`MountError` variants: `NotAMount`, `UnknownBlock`, `Read`, `Parse`.
They surface as `AppError::Mount(UiError)` in app state.
