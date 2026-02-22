# Message Routing

For overall architecture and document index, see [architecture.md](architecture.md).

## Goal

Keep `update` readable while preserving Rust's compile-time exhaustiveness checks.

## Message Structure

Top-level `Message` is a domain router in `src/app.rs`.

| Domain | Enum | Typical variants |
|---|---|---|
| Undo/redo | `UndoRedoMessage` | `Undo`, `Redo` |
| Text editing | `EditMessage` | `PointEdited { block_id, action }` |
| Shortcuts | `ShortcutMessage` | `Trigger`, `ForBlock` |
| Reduce flow | `ReduceMessage` | `Start`, `Cancel`, `Done`, `Apply`, `Reject` |
| Expand flow | `ExpandMessage` | `Start`, `Cancel`, `Done`, `ApplyRewrite`, child accept/reject |
| Tree structure | `StructureMessage` | `AddChild`, `AddSibling`, `DuplicateBlock`, `ArchiveBlock`, `ToggleFold` |
| Overlay | `OverlayMessage` | `ToggleOverflow`, `CloseOverflow` |
| Mount/file/theme | `MountFileMessage` | mount expand/collapse, save/load picked, `SystemThemeChanged` |

## Routing Pattern

1. `update(state, message)` delegates to `AppState::dispatch_message(message)`.
2. `dispatch_message` matches the top-level `Message` by domain.
3. Each domain handler matches its sub-enum exhaustively.

This keeps routing thin and domain logic local.

## Why This Design

- Exhaustiveness is preserved at both levels: top-level domain and per-domain variants.
- Handlers take strongly typed payloads, reducing accidental cross-domain coupling.
- Refactors are safer: adding a variant fails compilation in one focused handler.
- Tests become clearer because message intent is explicit.

## Conventions

- Add new variants to the domain enum that owns the state transition.
- Avoid catch-all (`_`) arms in domain handlers.
- Prefer struct-like variants for multi-field payloads (`Done { ... }`).
- Keep `update` and `dispatch_message` as routing only; business logic belongs in domain handlers.
