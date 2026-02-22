# Persistence Flow

For overall architecture and document index, see [architecture.md](architecture.md).

## Overview

Persistence is split into two layers:

- Main document file (`blocks.json`) under `AppPaths::data_dir()`.
- Mounted external files referenced by `BlockNode::Mount { path }`.

`AppState::save_tree()` is the single persistence entry point in app logic.
It writes the main file first, then writes every expanded mount file.

## Startup Load

`AppState::load()` calls `BlockStore::load()`:

1. Resolve `<data_dir>/blocks.json` via `AppPaths::data_file()`.
2. Read JSON and deserialize `BlockStore`.
3. If the file is missing, start from `BlockStore::default()`.
4. If data path resolution fails, read fails for other reasons, or JSON
   is malformed, startup enters a guarded state:
   - In-memory store starts from `BlockStore::default()`.
   - UI shows a persistence error banner.
   - Further saves are blocked for the session to avoid overwriting data.

Mount table metadata is runtime-only (`#[serde(skip)]`) and starts empty.
Mounts are reconstructed lazily when users expand mount nodes.

## Save Pipeline

`AppState::save_tree()` runs this sequence:

1. `BlockStore::save()` writes the main document snapshot.
2. `BlockStore::save_mounts()` writes all expanded mounted sub-stores.

This function is called after point edits, structure edits, draft updates,
undo/redo restores, and mount actions.

If persistence is blocked due to startup load failure, `save_tree()` returns
an error and keeps the banner visible instead of writing files.

## Main Document Save (`BlockStore::save`)

Before serialization, `snapshot_for_save()` builds a compact snapshot:

- Excludes re-keyed blocks that were loaded from mounted files.
- Restores expanded mount points back to `BlockNode::Mount { rel_path }`.
- Remaps persisted draft keys to the compacted key-space.

Result: the main file stores mount references, not mounted inline content.

## Mounted File Save (`BlockStore::save_mounts`)

For each `MountEntry` in `MountTable`:

- Extract mounted blocks into a standalone `BlockStore`.
- Collapse expanded nested mount points back to `Mount { path }` in the
  serialized parent file.
- Preserve draft records for mounted blocks.
- Serialize and write to the mount entry's canonical absolute path.

Nested relative mount paths resolve against their parent mount file
directory (not globally against the app data directory).

## Load/Save-To-File UI Actions

`Load from file` (`Message::LoadFromFilePicked`):

1. Ensure target block exists and has no children.
2. Replace node with `BlockNode::Mount { rel_path }`.
3. Expand the mount immediately.
4. Persist through `save_tree()`.

`Save to file` (`Message::SaveToFilePicked`):

1. Extract the block's children subtree into a standalone store file.
2. Replace the block with `BlockNode::Mount { rel_path }`.
3. Re-expand immediately for uninterrupted UI.
4. Persist through `save_tree()`.

## Serialization Error Policy

Serialization is strict:

- If `serde_json::to_string_pretty(...)` fails for main or mounted saves,
  the save returns an error and nothing is written for that save target.
- There is no `{}` fallback payload.

This avoids silently writing structurally-valid but semantically-wrong files.
