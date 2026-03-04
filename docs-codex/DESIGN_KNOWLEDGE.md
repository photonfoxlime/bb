# Blooming Blockery — Design Knowledge

## Architecture

### Architectural style

The GUI is a conventional iced Elm architecture: `iced::application(AppState::load, AppState::update, AppState::view)` creates the app, `AppState` owns all mutable state, `Message` is the single top-level event enum, and feature modules implement focused handlers that mutate `AppState` and optionally return `Task<Message>`. Source: `src/lib.rs:120-123`, `src/app.rs:204-235`, `src/app.rs:237-367`.

The overall repository is not GUI-only. It is one shared block-store core with two front ends:

- GUI: `blooming-blockery` for interactive document work.
- CLI: `bb` for structured automation and inspection.

Sources: `src/lib.rs:55-123`, `src/cli/commands.rs:58-118`.

### State ownership split

- Durable document state: `BlockStore` plus mounted file metadata and persisted per-block UI hints. Source: `src/store.rs:1-61`, `src/store.rs:233-260`.
- Durable app preferences: `AppConfig`. Source: `src/app/config.rs:30-128`.
- Durable provider credentials: `LlmProviders`. Source: `src/llm/config.rs:1-260`.
- Transient UI state: focus, mode, overlays, multiselect, hover, cursor, link candidates. Source: `src/app.rs:879-966`.
- Widget-local state: `EditorBuffers`, including text editor contents and widget ids. Source: `src/app.rs:167-201`, `src/app.rs:624-807`.
- In-flight async request state: `LlmRequests`. Source: `src/app/llm_requests.rs:1-137`.
- Undo history: `UndoHistory<UndoSnapshot>`, where snapshots include `store` and `navigation` but not editor buffers. Source: `src/app.rs:967-1064`.

### End-to-end data flow

1. Startup loads provider config, app config, and the main store; load failures enter guarded recovery mode instead of overwriting possibly recoverable data. Source: `src/app.rs:497-541`.
2. The view layer renders either document or settings mode from borrowed state only. Source: `src/app.rs:361-367`.
3. User input enters through either the global subscription layer or per-widget callbacks. Source: `src/app.rs:372-483`, `src/component/point_text_editor.rs:170-220`.
4. `AppState::update` routes the top-level `Message` into a focused handler module. Source: `src/app.rs:237-357`.
5. Structural changes mutate the store, snapshot undo state, and persist if allowed. Source: `src/app.rs:762-799`.
6. Async LLM and file-dialog work returns through `Task<Message>` completions and re-enters `update`. Source: `src/app/patch.rs:156-180`, `src/app/instruction_panel.rs:147-181`, `src/app/mount_file.rs:84-218`.

### Module boundary map

High-level map:

- `src/store*`: domain model, persistence, search, mounts, context assembly.
- `src/llm*`: provider config, prompt contracts, HTTP client.
- `src/app*`: GUI shell, handlers, screens, overlays.
- `src/component*`: GUI helper widgets with no app-specific state ownership.
- `src/cli*`: command parsing and execution over the same store.
- `src/theme.rs`, `src/text.rs`, `src/i18n.rs`, `src/paths.rs`, `src/undo.rs`: shared utilities.

Sources: `src/lib.rs:29-38`, `src/app.rs:48-76`, `src/cli.rs:34-47`, `src/store.rs:63-69`, `src/llm.rs:12-16`.

### Rewrite map

See `SUBSYSTEM_MAP.md` for the rewrite order and subsystem dependency table.

## Message Hierarchy

### Top-level message tree

```text
Message
├── UndoRedo(UndoRedoMessage)
├── Edit(EditMessage)
├── Shortcut(ShortcutMessage)
├── Error(ErrorMessage)
├── Patch(PatchMessage)
├── Structure(StructureMessage)
├── Find(FindMessage)
├── Overlay(OverlayMessage)
├── MountFile(MountFileMessage)
├── FriendPanel(FriendPanelMessage)
├── InstructionPanel(BlockId, InstructionPanelMessage)
├── Settings(SettingsMessage)
├── WindowResized(WindowSize)
├── KeyboardModifiersChanged(Modifiers)
├── DocumentMode(DocumentMode)
├── SystemThemeChanged(Mode)
├── Navigation(NavigationMessage)
├── ContextMenu(ContextMenuMessage)
├── LinkMode(LinkModeMessage)
├── LinkChipToggle(BlockId)
├── MultiselectBlockClicked(BlockId)
├── MultiselectBackspace
├── CursorPosition(Point)
└── EscapePressed
```

Source: `src/app.rs:204-235`.

### Notable nested message enums

- `UndoRedoMessage::{Undo, Redo}`. Source: `src/app.rs:1038-1063`.
- `PatchMessage::{Start, Cancel, Done, ApplyRewrite, RejectRewrite, AcceptChild, RejectChild, AcceptAllChildren, DiscardAllChildren}`. Source: `src/app/patch.rs:38-76`.
- `FindMessage::{Toggle, Open, Close, Escape, QueryChanged, DebounceElapsed, JumpSelected, JumpNext, JumpPrevious, JumpToIndex}`. Source: `src/app/find_panel.rs:114-137`.
- `LinkModeMessage::{Enter, QueryChanged, Confirm, SelectPrevious, SelectNext, Cancel}`. Source: `src/app.rs:831-845`.
- `NavigationMessage::{Enter, GoTo, Home}`. Source: `src/app/navigation.rs:47-70`.
- `FriendPanelMessage::{Toggle, StartFriendPicker, StartEditingFriendPerspective, CancelEditingFriendPerspective, UpdateFriendPerspectiveInput, ClearFriendPerspective, AcceptFriendPerspective, ToggleParentLineageTelescope, ToggleChildrenTelescope, HoverFriend, UnhoverFriend}`. Source: `src/app/friends_panel.rs:40-73`.
- `InstructionPanelMessage::{Toggle, TextEdited, Probe, ProbeChunk, ProbeFailed, ProbeFinished, CancelProbe, AmplifyWithInstruction, DistillWithInstruction, ApplyInstructionRewrite, AppendInstructionResponse, AddInstructionResponseAsChild, Dismiss}`. Source: `src/app/instruction_panel.rs:58-91`.
- `MountFileMessage::{ExpandMount, CollapseMount, SaveToFile, SaveToFilePicked, LoadFromFile, LoadFromFilePicked, MoveMount, MoveMountPicked, InlineMount, CancelInlineMountAllConfirm, InlineMountAll}`. Source: `src/app/mount_file.rs:20-47`.
- `ShortcutMessage::{Trigger, ForBlock, Movement}`. Source: `src/app/shortcut.rs:31-37`.

### Message origin categories

- User interaction: edit, context menu, shortcut, friends, settings, mount actions, panel toggles.
- Async completions: patch `Done`, mount file picker `*Picked`, probe stream chunk/final/error.
- Global subscriptions: window resize, keyboard modifiers, theme changes, cursor position, escape, find shortcuts, movement shortcuts, link-candidate navigation.

Sources: `src/app.rs:237-357`, `src/app.rs:372-483`, `src/app/patch.rs:156-180`, `src/app/instruction_panel.rs:147-181`, `src/app/mount_file.rs:84-218`.

## UI/UX Screen Inventory

### Screen: Document

- Purpose: Primary tree editor for writing, restructuring, searching, mounting, and invoking LLM workflows.
- Layout: `column![].spacing(theme::LAYOUT_GAP)` containing a scrollable document tree centered within a paper-like canvas; top-left mode bar; top-right help/undo/redo/settings controls; overlays stacked above the document. Source: `src/app/document.rs:118-235`.
- Key widgets:
  - Mode buttons for normal, find, link, multiselect. Source: `src/app/document.rs:123-205`.
  - Scrollable `TreeView` roots with per-row action bars and panels. Source: `src/app/document.rs:220-235`.
  - Help button, settings button, undo/redo buttons. Source: `src/app/document.rs:237-260`.
- Navigation: Opens settings, toggles overlays, dispatches row-level messages, and changes `DocumentMode`.
- Visual conventions: Paper-and-ink palette, centered readable canvas width, typography via LXGW WenKai + Inter, lucide iconography. Sources: `src/theme.rs:15-95`, `src/lib.rs:107-118`.
- Shortcuts: global find, undo/redo, action shortcuts, movement shortcuts, Backspace in multiselect. Sources: `src/app.rs:372-483`, `src/app/shortcut.rs:4-103`.

### Screen: Settings

- Purpose: Configure providers, per-task model/prompt settings, locale, appearance, Enter behavior, and inspect data paths.
- Layout: Centered scrollable form screen using the same max-width canvas constraint as the document view. Source: `src/app/settings.rs:1-38` and the module-level view docs near `src/app/settings.rs:803-807`.
- Key widgets: provider picker and form, task sections, appearance slider, locale controls, read-only path display.
- Navigation: Opened from the document gear button; dismissed back to document view.
- Visual conventions: Same theme tokens as the document canvas; form-driven rather than tree-driven layout.

### Overlay: Find panel

- Purpose: Search block points and jump between matches.
- Layout: Floating panel with title, query input, result list, and count / helper text. Source: `src/app/find_panel.rs:139-214`.
- Key widgets: text input, selectable match rows, next/previous keyboard navigation.
- Visibility: Only when `DocumentMode::Find`.

### Overlay: Link panel

- Purpose: Search the filesystem and convert an empty text point into a typed link.
- Layout: Floating panel with title row, search input, candidate list, and hint text. Source: `src/app/link_panel.rs:131-220`.
- Key widgets: search input with auto-focus, candidate buttons, close button.
- Visibility: Only when `DocumentMode::LinkInput`.

### Panel: Friends

- Purpose: Attach extra context blocks to the focused block and edit per-friend perspective / telescope flags.
- Layout: Inline panel below the focused row, sharing the same visual pattern as draft panels. Source: `src/app/friends_panel.rs:1-215`.
- Key widgets: add button, friend rows, remove controls, telescope toggles, inline perspective editor.
- Visibility: Controlled by persisted `BlockPanelBarState::Friends`.

### Panel: Instruction

- Purpose: Author instruction drafts, stream probe replies, and launch instruction-guided amplify/distill.
- Layout: Inline panel below the focused row with a text editor and action buttons. Source: `src/app/instruction_panel.rs:1-180`.
- Key widgets: instruction editor, probe / amplify / distill buttons, apply / append / add-child actions for probe responses.
- Visibility: Controlled by persisted `BlockPanelBarState::Instruction`.

### Panel: Patch drafts

- Purpose: Review amplify / atomize / distill results before applying them to the document.
- Layout: Inline diff / child-list panel rendered under the target block. Source: `src/app/patch.rs:1-76` and `src/component/patch_panel.rs`.
- Key widgets: rewrite accept/reject controls, per-child accept/reject controls, accept-all / discard-all actions.

### Overlay / banner: Error banner and shortcut help

- Purpose: Surface recoverable errors and keep shortcut discovery in-app.
- Layout: Non-modal overlays in the document view. Source: `src/app/error_banner.rs:1-85`, `src/app/document.rs:237-260`, module docs in `src/app/document.rs:64-71`.

## Design Decision Log

### Decision: Separate durable store state from transient UI state

- Alternatives: Put focus, hover, overlays, and selection directly on `BlockStore` or inside the same undo snapshot payload.
- Reason: Explicitly documented to keep serialization lean and avoid polluting undo with non-semantic UI state. Source: `src/app.rs:879-966`.
- Impact: Easier persistence and safer undo; more adapter code between store mutations and UI mutations.

### Decision: Include navigation in undo snapshots, but rebuild editor buffers

- Alternatives: Exclude navigation from undo, or snapshot editor cursor state as well.
- Reason: The code comments explicitly treat navigation as semantic view state but editor cursor state as cheap-to-rebuild widget state. Source: `src/app.rs:967-1026`.
- Impact: Undo cannot strand the user inside an invalid subtree, but caret position may reset after undo.

### Decision: Use typed point content instead of raw strings only

- Alternatives: Keep all points as `String` and infer link behavior elsewhere.
- Reason: The store now supports text and links with backward-compatible serde while keeping most call sites string-friendly. Source: `src/store/point.rs:1-170`.
- Impact: Link rendering and conversion are first-class, but persistence and UI code must branch on `PointContent`.

### Decision: Treat mounted files as runtime-expanded projections

- Alternatives: Always inline mounted content, or make each file its own separate app state.
- Reason: The mount system deliberately loads external files into the main graph with fresh ids, then projects them back out on save. Source: `src/store/mount.rs:1-240`, `src/store/persist.rs:40-84`.
- Impact: Users get one navigable document space across files, at the cost of a more complex persistence model.

### Decision: Prefer abortable async tasks with stale-response checks for LLM work

- Alternatives: Fire-and-forget requests or last-write-wins updates without request signatures.
- Reason: Patch and probe handlers record request signatures and abort handles so late responses do not overwrite newer edits. Sources: `src/app/llm_requests.rs:1-137`, `src/app/patch.rs:101-180`, `src/app/instruction_panel.rs:123-180`.
- Impact: More request bookkeeping, but safer asynchronous behavior under active editing.

### Decision: Split global shortcut handling from editor-local bindings

- Alternatives: Put all shortcuts in the global subscription or all of them in the editor widget.
- Reason: The code explicitly excludes some actions from global routing to avoid duplicate dispatch while keeping others globally reliable. Sources: `src/app.rs:485-494`, `src/component/point_text_editor.rs:170-220`.
- Impact: Shortcut logic is more deliberate and less fragile, but rewrite work must preserve the split carefully.

### Decision: Keep link mode filesystem-only and store absolute paths

- Alternatives: URL support, relative-path links, or asynchronous indexing.
- Reason: Documented in the module-level design rationale for `link_panel`. Source: `src/app/link_panel.rs:22-31`.
- Impact: Simpler implementation and preview loading, but lower portability and no broken-link diagnostics.

### Decision: Use prompt/task-specific provider and model settings

- Alternatives: One global provider/model for every LLM action.
- Reason: `TaskKind` and `TaskConfig` are designed so each workflow can choose its own provider, model, token limit, and prompts. Sources: `src/app/config.rs:14-22`, `src/llm/config.rs:78-157`.
- Impact: Greater flexibility for power users; larger settings surface.

## Public API Surface

This crate is primarily a binary/application crate, not a general-purpose library. The intentionally public surface is small:

- `BloomingBlockery` runtime entry namespace. Source: `src/lib.rs:49-123`.
- `text` module. Source: `src/lib.rs:29-38`.
- CLI command modules, result formatting, and some store/LLM domain types are `pub` inside the crate, but they are mainly consumed by the app binaries rather than by external downstream crates. Sources: `src/cli.rs:34-47`, `src/llm.rs:12-24`.

If you are rewriting, treat the public API as incidental compared with the internal architecture and persistence contracts.
