# Blooming Blockery — Functionality Inventory

## Purpose

Blooming Blockery is a tree-structured document editor for designers and developers. Its core job is to let users build and reorganize idea trees, enrich or condense nodes with LLM assistance, and optionally project subtrees into external files without leaving the same document model. The same `BlockStore` powers both the iced GUI and the `bb` CLI, so interactive editing and scripted automation operate on one persisted format rather than separate products. Sources: `README.md:1-36`, `DESIGN.md:1-35`, `src/lib.rs:55-92`, `src/store.rs:1-61`.

## Feature List

### Feature: Tree-structured document editing

- Description: The GUI renders a forest of blocks, each with one point and zero or more children. Users can edit block text directly, focus rows, and keep the tree visible in one scrollable document canvas.
- Entry point: `blooming-blockery` GUI startup via `BloomingBlockery::run_gui` and `AppState::view`; document rendering starts in `DocumentView::view`.
- Status: fully implemented.
- Notes: The store guarantees at least one root block and keeps structure and point content in separate maps. Sources: `src/lib.rs:97-123`, `src/app.rs:361-367`, `src/app/document.rs:118-235`, `src/store.rs:233-260`.

### Feature: Structural tree mutations

- Description: Users can add children, add siblings, wrap a block with a new parent, duplicate subtrees, delete subtrees, and move blocks before/after/under another block.
- Entry point: GUI action bar and keyboard shortcuts dispatch into `StructureMessage` / `EditMessage`; CLI exposes the same operations under `bb tree ...`.
- Status: fully implemented.
- Notes: Removal cleans up drafts, friend references, panel state, and mount origins; moving rejects ancestor-into-descendant cycles. Sources: `src/store/tree.rs:19-233`, `src/cli/tree.rs:10-176`, `src/app/action_bar.rs:37-64`.

### Feature: Undo / redo

- Description: Users can undo and redo semantic document mutations. Undo snapshots include both store state and navigation position.
- Entry point: `Cmd/Ctrl+Z`, `Cmd/Ctrl+Shift+Z`, top-right toolbar buttons, and context menu actions.
- Status: fully implemented.
- Notes: Editor buffer cursor state is intentionally rebuilt rather than snapshotted. Sources: `src/app.rs:445-450`, `src/app.rs:969-1064`, `src/app/document.rs:257-260`.

### Feature: Keyboard-driven navigation and reordering

- Description: Users can move focus across siblings and parent/child relationships, and can reorder / indent / outdent blocks from the keyboard with wrap-around semantics.
- Entry point: Global shortcut subscription plus editor key binding; parser lives in `movement_shortcut_from_key`.
- Status: fully implemented.
- Notes: macOS uses `Ctrl+Arrow`, other platforms use `Alt+Arrow`; collapsed ancestors are unfolded automatically before focus jumps into hidden content. Sources: `src/app.rs:372-483`, `src/app/shortcut.rs:4-103`, `src/app/shortcut.rs:173-260`.

### Feature: LLM amplify / distill / atomize workflows

- Description: Users can ask the model to elaborate a block, condense it, or split it into distinct information points. Results are staged as drafts before acceptance.
- Entry point: Per-block action bar, action shortcuts, patch panel.
- Status: fully implemented.
- Notes: Each request captures a context signature and discards stale responses after document changes; requests are abortable. Sources: `src/app/patch.rs:1-180`, `src/app/llm_requests.rs:1-260`, `src/llm/client.rs:44-239`.

### Feature: Instruction-driven probe workflow

- Description: Users can author a block-specific instruction, stream a free-form probe response from the model, then replace, append, or insert that response as a child.
- Entry point: Instruction panel under a focused block.
- Status: fully implemented.
- Notes: Probe prefers streaming SSE and falls back to one-shot completion when streaming is unsupported or silent. Sources: `src/app/instruction_panel.rs:1-180`, `src/llm/client.rs:242-321`, `src/llm/client.rs:387-420`.

### Feature: Friend blocks as extra LLM context

- Description: Users can attach arbitrary related blocks to a target block, optionally add a perspective string, and toggle whether each friend contributes parent lineage and/or children to LLM context.
- Entry point: Friends panel under a focused block.
- Status: fully implemented.
- Notes: Friend blocks are not structural children; they enrich `BlockContext` only. Sources: `src/store.rs:88-117`, `src/store/drafts.rs:251-260`, `src/store/navigate.rs:79-138`, `src/app/friends_panel.rs:1-180`.

### Feature: Global find

- Description: Users can search block points across the whole store and jump through matches in DFS order.
- Entry point: `Cmd/Ctrl+F` to open, `Cmd/Ctrl+G` / `Cmd/Ctrl+Shift+G` to cycle, floating overlay UI.
- Status: fully implemented.
- Notes: Query updates are debounced and use phrase-aware matching instead of plain substring-only search. Sources: `src/app/find_panel.rs:1-214`, `src/store/navigate.rs:11-44`.

### Feature: Link blocks with inline previews

- Description: Users can convert a point into a typed link block, render it as a chip, and expand inline previews for images and markdown files.
- Entry point: Type `@` in an empty point editor or use the context menu convert action.
- Status: fully implemented.
- Notes: Current link search is filesystem-only, synchronous, and stores absolute paths; broken links are not proactively validated. Sources: `src/app/link_panel.rs:1-220`, `src/store/point.rs:1-170`, `src/component/point_text_editor.rs:95-166`, `src/app.rs:224-235`.

### Feature: Mount external files

- Description: Users can convert a block into a mount point, expand it into live children, collapse it back to a file reference, move the backing file, inline one mount, inline nested mounts recursively, or extract an existing subtree to a file.
- Entry point: GUI mount overflow actions and file dialogs; CLI `bb mount ...`.
- Status: fully implemented.
- Notes: Supported formats are JSON and Markdown Mount v1. Expanded mounts are runtime-only and are re-projected during persistence. Sources: `src/store/mount.rs:1-240`, `src/store/persist.rs:40-84`, `src/store/markdown.rs:1-162`, `src/app/mount_file.rs:1-220`, `src/cli/mount.rs:10-192`.

### Feature: Drill-down navigation and breadcrumbs

- Description: Users can navigate into a block so its children become the visible root set, then return through breadcrumbs or home.
- Entry point: Action-bar overflow action and breadcrumb UI.
- Status: fully implemented.
- Notes: Navigation state participates in undo/redo and preserves optional mount path hints for breadcrumbs. Sources: `src/app/navigation.rs:1-220`, `src/app/document.rs:220-260`, `src/app/action_bar.rs:58-64`.

### Feature: Multiselect deletion mode

- Description: Users can switch into a read-only selection mode, select multiple blocks, and delete the selection with Backspace.
- Entry point: Mode bar toggle in document view.
- Status: fully implemented.
- Notes: Current scope is intentionally narrow: batch deletion only, no multi-drag or multi-action bar yet. Sources: `src/app.rs:294-335`, `src/app/document.rs:123-205`, `src/app/multiselect.rs:1-63`.

### Feature: Settings / provider management

- Description: Users can configure preset and custom LLM providers, per-task models and token limits, custom prompts, locale, appearance, Enter-key behavior, and inspect resolved data/config paths.
- Entry point: Settings gear button.
- Status: fully implemented.
- Notes: Provider credentials live in `llm.toml`; per-task runtime preferences live in `app.toml`. Sources: `src/app/settings.rs:1-220`, `src/app/config.rs:1-220`, `src/llm/config.rs:1-260`, `src/paths.rs:19-40`.

### Feature: CLI automation surface

- Description: The `bb` binary exposes query, point editing, tree editing, navigation, draft manipulation, fold control, friend management, mount control, panel state, context inspection, and shell completion generation.
- Entry point: `BloomingBlockery::run_cli` and `Commands::execute`.
- Status: fully implemented.
- Notes: Most command families support batch targeting via comma-separated IDs; output can be table or JSON. Sources: `src/lib.rs:55-95`, `src/cli/commands.rs:1-118`, `src/cli/execute.rs:16-141`, `src/cli.rs:1-171`.

### Feature: Persistent, privacy-local document storage

- Description: The app stores its main document and settings in platform-specific local directories and does not include any built-in telemetry.
- Entry point: automatic load/save at startup and after successful mutations.
- Status: fully implemented.
- Notes: LLM providers may still collect request data; that is outside the app boundary. Sources: `README.md:39-41`, `src/paths.rs:1-40`, `src/store/persist.rs:23-84`, `src/app.rs:497-541`.

## Operational Modes

### Mode: GUI runtime

- Trigger: Launch `blooming-blockery`.
- Behavior differences: Starts the iced application, loads fonts and icon assets, subscribes to keyboard/window/theme events, and persists through `AppState`. Sources: `src/lib.rs:97-123`, `src/app.rs:372-483`.

### Mode: CLI runtime

- Trigger: Launch `bb`.
- Behavior differences: Parses clap commands, mutates the same store format, prints table/JSON output, and skips GUI-only transient state. Sources: `src/lib.rs:55-95`, `src/cli/commands.rs:58-118`, `src/cli/execute.rs:16-40`.

### Mode: Document view

- Trigger: Default GUI view or `SettingsMessage::Close`.
- Behavior differences: Renders the tree editor, overlays, action bars, and document modes. Sources: `src/app.rs:361-367`, `src/app/document.rs:118-260`.

### Mode: Settings view

- Trigger: Settings gear button or `SettingsMessage::Open`.
- Behavior differences: Renders provider/task/system configuration instead of the document tree. Sources: `src/app.rs:361-367`, `src/app/settings.rs:1-38`.

### Mode: Normal document interaction

- Trigger: Default document mode or explicit mode reset.
- Behavior differences: Standard text editing and per-block actions are enabled. Sources: `src/app.rs:294-319`, `src/app.rs:820-836`.

### Mode: Find

- Trigger: `Cmd/Ctrl+F`, mode toggle, or `FindMessage::Open`.
- Behavior differences: Shows the floating search panel and routes `Cmd/Ctrl+G` through current match navigation. Sources: `src/app/find_panel.rs:114-214`.

### Mode: PickFriend

- Trigger: Friends panel add action.
- Behavior differences: Document rows become friend-picking targets instead of plain edit targets. Sources: `src/app/friends_panel.rs:95-123`, `src/app/document.rs:848-990`.

### Mode: Multiselect

- Trigger: Multiselect mode button.
- Behavior differences: Blocks render as plain text rows for click selection and Backspace-driven deletion. Sources: `src/app.rs:294-335`, `src/app/document.rs:167-205`.

### Mode: LinkInput

- Trigger: Type `@` into an empty point editor or activate link mode on the focused block.
- Behavior differences: Opens a floating filesystem search panel; arrow keys navigate candidates. Sources: `src/app/link_panel.rs:53-129`, `src/app.rs:387-413`.

### Mode: Persistence recovery

- Trigger: `blocks.json` cannot be safely loaded.
- Behavior differences: Opens a temporary recovery workspace, records an error banner, and blocks save-through for the session. Sources: `src/app.rs:497-541`, `src/app.rs:605-639`.

### Mode: Appearance override vs system-follow

- Trigger: `app.toml` dark mode preference.
- Behavior differences: When no persisted override exists, system theme change events update the UI; otherwise they are ignored. Sources: `src/app.rs:337-350`, `src/app/config.rs:40-97`.

## Data & I/O

### Inputs

- Main store JSON: `<data_dir>/blocks.json`. Source: `src/paths.rs:19-29`, `src/store/persist.rs:23-38`.
- App preferences TOML: `<config_dir>/app.toml`. Source: `src/paths.rs:36-40`, `src/app/config.rs:99-128`.
- LLM provider TOML: `<config_dir>/llm.toml`. Source: `src/paths.rs:31-34`, `src/llm/config.rs:20-33`.
- Mounted subtree files: JSON or Markdown Mount v1. Source: `src/store/mount.rs:159-240`, `src/store/markdown.rs:1-162`.
- User keyboard/mouse input and file dialogs. Source: `src/app.rs:372-483`, `src/app/mount_file.rs:84-218`.
- Filesystem browsing for link mode, starting from `$HOME`. Source: `src/app/link_panel.rs:1-220`.
- Network requests to configured LLM endpoints over OpenAI-compatible or Anthropic-compatible APIs. Source: `src/llm/config.rs:40-157`, `src/llm/client.rs:325-420`.
- Environment overrides: `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`. Source: `src/llm/config.rs:32-33`.

### Outputs

- Persisted main store JSON and mounted JSON/markdown files. Source: `src/store/persist.rs:40-84`.
- Persisted app and provider TOML files. Source: `src/app/config.rs:120-128`, `src/llm/config.rs:20-33`.
- CLI stdout in table or JSON formats, plus shell completion scripts. Source: `src/lib.rs:61-91`, `src/cli.rs:146-153`.
- LLM requests and streamed probe chunks. Source: `src/app/patch.rs:156-180`, `src/app/instruction_panel.rs:123-180`, `src/llm/client.rs:242-321`.
- UI-only output: tree canvas, overlays, draft panels, previews, banners, settings forms. Source: `src/app/document.rs:118-260`, `src/app/find_panel.rs:139-214`, `src/app/link_panel.rs:131-220`, `src/app/settings.rs:1-38`.

### Persistence Between Runs

- Persisted: block graph, typed point content, drafts, friend relations, collapsed state, per-block panel state, mounted subtree references, app settings, provider configs. Sources: `src/store.rs:1-61`, `src/store/drafts.rs:1-260`, `src/app/config.rs:30-220`, `src/llm/config.rs:1-260`.
- Not persisted: focus, overlays, multiselect selection, link panel candidates, in-flight request state, hover state, editor cursor state. Sources: `src/app.rs:879-966`, `src/app/llm_requests.rs:1-137`.

## Implemented Algorithms

### Algorithm / Logic: Phrase-aware block search

- Location: `BlockStore::find_block_point`, `src/store/navigate.rs:11-44`.
- Purpose: Match point text by full-query substring or extracted phrase tokens, returning blocks in DFS order.
- Complexity / notes: Traverses all roots depth-first; empty queries match all blocks.

### Algorithm / Logic: LLM context assembly

- Location: `BlockStore::block_context_for_id_with_friend_blocks`, `src/store/navigate.rs:94-138`.
- Purpose: Build one context envelope from lineage, direct children, and friend blocks with optional telescope flags.
- Complexity / notes: Friend contexts can recursively include friend lineage and children.

### Algorithm / Logic: Stale-response rejection

- Location: `AppState::block_context_signature`, `AppState::is_stale_response`, `src/app.rs:800-807`; request lifecycle in `src/app/patch.rs:101-180` and `src/app/instruction_panel.rs:123-180`.
- Purpose: Drop LLM results that were computed against old document state.
- Complexity / notes: Signatures are captured before request dispatch and compared on completion.

### Algorithm / Logic: Mount re-keying and projection

- Location: `BlockStore::expand_mount`, `src/store/mount.rs:228-284`; save projection in `src/store/persist.rs:40-84`.
- Purpose: Load external subtree files into the main store without `BlockId` collisions, then persist them back out while preserving mount boundaries.
- Complexity / notes: Own-store blocks and mounted descendants are deliberately separated during save.

### Algorithm / Logic: Markdown Mount v1 render / parse

- Location: `src/store/markdown.rs:1-162`.
- Purpose: Encode / decode a projected subtree as an indented quoted bullet list with a required preamble.
- Complexity / notes: Strict parser; indentation jumps, missing preamble, and unsupported escapes are hard errors.

### Algorithm / Logic: Shortcut-driven sibling wrap and structural movement

- Location: `src/app/shortcut.rs:141-260`.
- Purpose: Provide cyclic sibling focus and movement semantics from the keyboard.
- Complexity / notes: Boundary behavior wraps instead of clamping; hidden ancestors are unfolded first.

### Algorithm / Logic: Link kind inference and inline preview selection

- Location: `PointLink::infer`, `src/store/point.rs:87-105`; rendering in `src/component/point_text_editor.rs:99-146`.
- Purpose: Infer whether a link should render as image, markdown, or plain path, then choose an inline preview strategy.
- Complexity / notes: Markdown preview reads from disk synchronously and degrades to an inline error string.

### Algorithm / Logic: Probe streaming fallback

- Location: `LlmClient::probe_stream`, `emit_inquiry_fallback`, `stream_inquiry_chunks`, `src/llm/client.rs:242-420`.
- Purpose: Prefer streaming completion, but fall back to one-shot completion when a provider does not support streaming or emits no chunks.
- Complexity / notes: Emits one terminal `Finished` event regardless of success or failure.

### Algorithm / Logic: Responsive action bar projection

- Location: `build_action_bar_vm` and `project_for_viewport`, `src/app/action_bar.rs:157-260`.
- Purpose: Build one action inventory from row state, then demote or surface actions based on viewport bucket without changing behavior.
- Complexity / notes: Availability is derived from row UI state rather than hand-coded per view branch.

## Unimplemented / Abandoned

- `todo!()`: not found in production source.
- `unimplemented!()`: not found in production source.
- `TODO` / `FIXME` / `HACK` / `NOTE` markers in production source: not found.
- The only `TODO` text match under `src/` is a doc-comment example string in `src/cli/query.rs:33`: `Example: \`bb find "TODO"\`.` This is not an implementation marker.
- Hidden or debug-only feature flags beyond optional tracing: not found. The only crate feature is `log`, which enables `tracing-subscriber`. Source: `Cargo.toml:31-33`.
