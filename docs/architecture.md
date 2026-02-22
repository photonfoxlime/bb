# Architecture

Overview of the bb codebase for contributors and AI assistants. Records module structure, key types, and design decisions so future sessions stay aligned.

For project purpose, data model, and workflow, see [README.md](../README.md) (canonical source).

## Document Index

| Document | Covers |
|----------|--------|
| [README.md](../README.md) | Project purpose, data model, workflow, UI concept |
| [architecture.md](architecture.md) | Module map, key types, design decisions (this file) |
| [action-bar.md](action-bar.md) | Action bar structure, shortcuts, responsive projection |
| [mount-system.md](mount-system.md) | External file mounts, save/load to file, cycle detection |
| [undo-system.md](undo-system.md) | Undo/redo architecture, snapshot protocol |
| [expansion-diff.md](expansion-diff.md) | Expansion draft diff rendering |
| [backlog.md](backlog.md) | Unimplemented ideas and exploration items |
| [keyboard.md](keyboard.md) | Keyboard navigation: block traversal, focus transfer |

## Product Principle

Prioritize clarity of thought structure over UI chrome.
Help users think in branches, compare alternatives, and progressively refine ideas.
Preserve tree readability first. Avoid timeline metaphors. Keep structural-spine concept.

## Aesthetic Direction

- Light, airy, calm. Soft blue-ink tone. Paper-like background. Generous whitespace.
- Structure visible through vertical spines and uniform dot markers, not decorative widgets.
- Actions feel like marginalia annotations, not toolbar chrome.
- Fonts: LXGW WenKai for point text (default), Inter for utility labels and buttons.
- Palette defined in `src/theme.rs`: PAPER (warm off-white), INK (near-black), ACCENT (soft blue), ACCENT_MUTED, TINT (warm gray), SPINE (low-contrast gray), SPINE_LIGHT (lighter structural line), DANGER, SUCCESS, FOCUS_WASH (faint accent for active-block highlight).

## Module Structure

- `src/main.rs` -- Iced app entry. Loads fonts (WenKai, Inter, Lucide), wires theme.
- `src/app.rs` -- Orchestration: AppState, Message enum, update loop, subscription, view dispatch.
- `src/app/state.rs` -- UI error types and async lifecycle enums (UiError, AppError, SummaryState, ExpandState).
- `src/app/draft.rs` -- ExpansionDraft: pending expand result (rewrite + child suggestions).
- `src/app/editor_store.rs` -- EditorStore: SecondaryMap\<BlockId, text\_editor::Content\> for editor buffers, plus SecondaryMap\<BlockId, widget::Id\> for programmatic focus.
- `src/app/view.rs` -- TreeView: pure renderer from immutable AppState into widget tree.
- `src/app/action_bar/` -- Typed action bar: types, selector (state-to-VM), responsive projection, keyboard shortcuts, dispatch.
- `src/store.rs` -- Block store data model: BlockId (slotmap key), BlockNode enum, BlockStore (SlotMap + SecondaryMaps). JSON persistence.
- `src/mount.rs` -- Mount table: MountTable, MountEntry, BlockOrigin, MountError. Tracks blocks loaded from external files.
- `src/paths.rs` -- Shared application directory paths (AppPaths).
- `src/undo.rs` -- Generic undo/redo history (UndoHistory\<T\>).
- `src/llm.rs` -- LLM client, config loading (env vars + TOML file), prompt construction, expand/summarize API.
- `src/theme.rs` -- Custom paper-and-ink theme: palette constants, layout tokens (spacing, sizing, padding), and per-widget style functions.

## Key Types

| Type | Location | Purpose |
|------|----------|---------|
| `BlockId` | `store.rs` | Slotmap key type (`new_key_type!`) for block identity. Copy, not UUID. |
| `BlockStore` | `store.rs` | Roots + SlotMap\<BlockId, BlockNode\> + SecondaryMap\<BlockId, String\> (points). JSON persistence. |
| `BlockNode` | `store.rs` | Enum: `Children { children: Vec<BlockId> }` or `Mount { path: PathBuf }`. Text stored separately in points map. |
| `MountTable` | `mount.rs` | Runtime-only table tracking block origins and mount entries. Not serialized. |
| `MountEntry` | `mount.rs` | Per-mount-point metadata: canonical path, relative path, root ids, block ids. |
| `BlockOrigin` | `mount.rs` | Enum: `Mounted { mount_point: BlockId }`. Tracks which mount loaded a block. |
| `MountError` | `mount.rs` | Error enum (via thiserror): NotAMount, UnknownBlock, CycleDetected, Read, Parse. |
| `AppPaths` | `paths.rs` | Data file and config file paths via `directories` crate. |
| `UndoHistory<T>` | `undo.rs` | Fixed-capacity undo/redo stack. |
| `UiError` | `app/state.rs` | Display-safe error for UI messages. |
| `AppError` | `app/state.rs` | Tagged application error source (config, summary, expand, mount). |
| `SummaryState` | `app/state.rs` | Per-row summarize lifecycle (Idle, Loading, Error). |
| `ExpandState` | `app/state.rs` | Per-row expand lifecycle (Idle, Loading, Error). |
| `ExpansionDraft` | `app/draft.rs` | Pending expand result: optional rewrite + child suggestions. |
| `EditorStore` | `app/editor_store.rs` | SecondaryMap\<BlockId, text\_editor::Content\> for editor buffers; SecondaryMap\<BlockId, widget::Id\> for programmatic focus targeting. |
| `AppState` | `app.rs` | Full UI state: store, editors, LLM config, lifecycle, drafts, focused/active block tracking, and `collapsed` set for fold state. Per-block maps use SecondaryMap. |
| `TreeView` | `app/view.rs` | Pure renderer: borrows immutable AppState, produces Element tree. |
| `LlmClient` | `llm.rs` | HTTP client for summarize and expand requests. |
| `LlmConfig` | `llm.rs` | base_url, api_key, model. Loaded from env vars or `llm.toml`. |

## AppState Block Selectors

- `active_block_id` -- Last interacted block for action dispatch and non-editor shortcut fallback.
- `focused_block_id` -- Block whose `text_editor` is currently focused; first target for shortcuts while typing.
- `editing_block_id` -- Undo coalescing marker for point edits; not a focus signal and not persisted in snapshots.

## Design Decisions

- **Slotmap-based addressing**. Block identity uses `slotmap::new_key_type!` (`BlockId`). Keys are generated by `SlotMap::insert` and are Copy. All per-block side maps (`expansion_drafts`, `summary_states`, `expand_states`, `errors`, `overflow_open`, `editor_store`) use `slotmap::SecondaryMap<BlockId, V>`.
- **Lineage-based context**. Summarize and expand use DFS root-to-target lineage as LLM context.
- **Single-block async lifecycle**. One summarize and one expand operation active at a time (`SummaryState` / `ExpandState` enums).
- **Draft-then-apply**. Expand results land in `ExpansionDraft` for review. Rewrite and each child accepted/rejected independently.
- **Pure renderer**. `TreeView` borrows immutable state, produces widgets. No mutation during rendering.
- **Lazy mount loading**. Mount nodes reference external files but are not loaded until the user expands them. See [mount-system.md](mount-system.md).

### Iced layout pitfall (keep in mind)

Iced's `center_x(width)` overrides the preceding `width(...)` call because it is implemented as `self.width(width).align_x(Center)`. Use `.align_x(Horizontal::Center)` instead to preserve explicit widths on containers.

## Visual Implementation

Layout tokens are defined in `src/theme.rs` and used throughout the view layer. All spacing, sizing, and padding values are named constants rather than magic numbers.

- Canvas: `CANVAS_PAD` (24px) outer padding, `CANVAS_MAX_WIDTH` (720px) content width, `CANVAS_TOP` (12px) top padding.
- Block gaps: `BLOCK_GAP` (10px) between siblings, `BLOCK_INNER_GAP` (4px) within a block, `ROW_GAP` / `ACTION_GAP` (6px) horizontal.
- Spine: `rule::vertical(1)` styled with `SPINE_LIGHT` via `spine_rule`, inside a `SPINE_WIDTH` (4px) container.
- Marker: disclosure chevron (▾/▸) for foldable blocks (children, mounts), bullet dot for leaf blocks. `MARKER_WIDTH` (12px) column, `MARKER_TOP` (3px) top padding for baseline alignment.
- Child indent: `INDENT` (16px) left padding.
- Draft panels: `PANEL_PAD_V` / `PANEL_PAD_H` (8/16px) internal padding, `PANEL_BUTTON_GAP` (8px) between buttons.
- Active block: focused or active block wrapped in `active_block` container style (`FOCUS_WASH` -- 6% accent overlay).

## Keyboard Navigation

See [keyboard.md](keyboard.md).
