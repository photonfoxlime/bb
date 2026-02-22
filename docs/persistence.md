# Persistence Flow

For overall architecture and document index, see [architecture.md](architecture.md).

## Overview

Persistence has two write targets:

1. Main document file (`blocks.json`) under `AppPaths::data_dir()`.
2. Mounted files referenced by `BlockNode::Mount { path }`.

`AppState::save_tree()` is the single app-level entry point. It runs:

1. `BlockStore::save()` for main file snapshot.
2. `BlockStore::save_mounts()` for expanded mount entries.

## Startup Load

`AppState::load()` calls `BlockStore::load()`:

- Missing `blocks.json` -> start from `BlockStore::default()`.
- Path/read/parse failure -> guarded mode:
  - in-memory store starts as default,
  - persistence error is shown in UI,
  - saves are blocked for the session (`persistence_blocked = true`).

Reasoning: guarded mode prevents accidental overwrite after a corrupted or unreadable startup state.

## Save Semantics

`save_tree()` is called after edits, structure changes, mount actions, and undo/redo restore.

- Serialization is strict (`serde_json::to_string_pretty` failures return errors).
- No fallback payload is written.
- Persistence errors are surfaced as `AppError::Persistence`.

## Snapshot Behavior

Main-file save uses `snapshot_for_save()` to avoid inlining mounted file content:

- Expanded mount points are restored to `Mount { rel_path }`.
- Mounted descendants are excluded from main snapshot.
- Draft keys are remapped to compacted ids; drafts for excluded mounted blocks are dropped.

Mounted-file save extracts each expanded mount subtree from live state and writes it back to the mount file. Expanded nested mounts are serialized as mount links (not inlined).

## Failure-Mode Matrix

| Stage | Failure | Runtime behavior | User impact | Data risk |
|---|---|---|---|---|
| Startup load | Path/read/parse error | Enter guarded mode, block future saves | Error banner, no save-through this session | Prevents overwrite of unknown/corrupt source |
| Main save (`save`) | IO/serialize error | `save_tree()` fails, mount saves are skipped | Error banner; mutation remains in memory | No on-disk update this call |
| Mount save (`save_mounts`) after successful main save | IO/serialize error for one mount | `save_tree()` fails after partial write | Error banner; main file may be newer than some mount files | Temporary cross-file skew until next successful save |

## Why main-first then mounts

Current order (`save` then `save_mounts`) prioritizes keeping the main graph shape current (including mount links). The tradeoff is possible temporary skew if a later mount write fails.

Operational guidance:

- Treat persistence errors as actionable and retry after fixing file-system conditions.
- Avoid forceful app termination immediately after a mount-save error.

## UI Actions and Persistence Hooks

- `Message::MountFile(MountFileMessage::LoadFromFilePicked { ... })`:
  convert block to mount path, expand immediately, then persist.
- `Message::MountFile(MountFileMessage::SaveToFilePicked { ... })`:
  extract subtree to file, replace with mount link, re-expand, then persist.
