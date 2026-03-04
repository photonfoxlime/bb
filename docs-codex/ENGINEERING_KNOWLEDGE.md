# Blooming Blockery — Engineering Knowledge

## Data Structures

### Core store and persistence types

| Name | Kind | Purpose | Key invariants / notes | Source |
| --- | --- | --- | --- | --- |
| `BlockId` | slotmap key newtype | Stable block identity inside one loaded store | Display format is `indexvgeneration`; ids are not file-stable across mount re-keying | `src/store.rs:117-126` |
| `BlockNode` | enum | Structural node, either inline children or a mount reference | `Children` nodes own ordered child ids; `Mount` nodes hold path + format and report empty `children()` | `src/store.rs:156-215` |
| `PointContent` | enum | Typed point payload (`Text` or `Link`) | Serializes text as bare string and links as object for backward compatibility | `src/store/point.rs:118-216` |
| `PointLink` / `LinkKind` | struct + enum | Typed external reference with inferred rendering kind | `href` non-empty by construction convention; `kind` inferred once on creation | `src/store/point.rs:32-105` |
| `FriendBlock` | struct | Per-target extra context relation | References another block plus optional perspective and telescope flags | `src/store.rs:88-117` |
| Draft record structs | structs | Persist pending LLM and instruction/probe state per block | Sparse maps; only present while work is pending or staged | `src/store/drafts.rs:10-70` |
| `BlockPanelBarState` | enum | Persist which inline panel is open for a block | Only `Friends` or `Instruction` persist | `src/store.rs:139-151` |
| `Direction` | enum | Relative move direction for tree mutations | `Before`, `After`, `Under` | `src/store.rs:129-137` |
| `BlockStore` | struct | Authoritative block forest plus optional metadata maps | Every id in roots / children should exist in both `nodes` and `points`; store always has at least one root | `src/store.rs:217-260` |
| `MountEntry`, `MountTable`, `MountFormat`, `MountError`, `BlockOrigin` | mount subsystem types | Track runtime-expanded mount ownership and save-back metadata | Runtime-only mount table is not serialized; nested mount ownership is tracked per block | `src/store/mount.rs:44-176` |
| `StoreLoadError` | enum | Main store load failure taxonomy | Distinguishes unavailable path, read failure, and parse failure | `src/store/persist.rs:13-21` |

### App-shell types

| Name | Kind | Purpose | Key invariants / notes | Source |
| --- | --- | --- | --- | --- |
| `AppState` | struct | Root GUI state | Splits durable store/config/providers from transient UI and widget buffers | `src/app.rs:121-201` |
| `Message` | enum | Top-level Elm event enum | Every GUI mutation funnels through `update` | `src/app.rs:204-235` |
| `DocumentMode` | enum | In-document interaction state | `Normal`, `Find`, `PickFriend`, `Multiselect`, `LinkInput` | `src/app.rs:810-830` |
| `ViewMode` | enum | Top-level screen switch | `Document` vs `Settings` only | `src/app.rs:862-871` |
| `TransientUiState` | struct | Non-persisted UI/session state | Cleared on reload; not included in undo | `src/app.rs:879-966` |
| `FocusState` | struct | Focused block + overflow state | Used as shortcut target and row UI state anchor | `src/app.rs:873-878` |
| `UndoSnapshot` | struct | Undo payload | Contains `store` + `navigation`, intentionally excludes editor buffers | `src/app.rs:967-1004` |
| `NavigationLayer` / `NavigationStack` | struct | Drill-down breadcrumb path | Paths are preserved for breadcrumb labels; stack participates in undo | `src/app/navigation.rs:72-220` |
| `FindUiState` | struct | Debounced search state | Selected index, when present, always points into `matches` | `src/app/find_panel.rs:24-112` |
| `LinkPanelState` | struct | Filesystem link-search state | Reset when link mode exits | `src/app.rs:847-860` |
| `LlmRequests` + `*State` enums | struct + enums | Track in-flight LLM lifecycle and abort handles per block | One abortable request per block per workflow; pending signatures gate stale results | `src/app/llm_requests.rs:1-137`, `src/app/llm_requests.rs:281-394` |

### Settings and LLM types

| Name | Kind | Purpose | Key invariants / notes | Source |
| --- | --- | --- | --- | --- |
| `AppConfig` | struct | Persisted UI and per-task LLM preferences | Locale and dark mode are optional overrides; task settings default when absent | `src/app/config.rs:30-97` |
| `TaskSettings`, `TaskConfig`, `MaxTokens` | structs | Per-task provider/model/prompt/token configuration | `0` tokens means unlimited | `src/app/config.rs:131-220` |
| `SettingsState`, `TaskDrafts`, `TaskDraft`, `ThemePreference`, `FirstLineEnterBehavior` | settings screen state | Non-destructive settings editing and UI-specific preference wrappers | Provider config and task config are intentionally edited separately | `src/app/settings.rs:57-220` |
| `ApiStyle`, `TaskKind`, `LlmConfig`, `PresetProvider`, `LlmProviders` | LLM config domain | Provider wire protocol, task categories, validated config resolution | `LlmConfig::from_raw` enforces base URL and non-empty api key/model | `src/llm/config.rs:40-260` |
| `BlockContext`, `LineageContext`, `ChildrenContext`, `FriendContext` | LLM context domain | Decouple prompt payloads from store ids | Context is immutable and serializable for request building | `src/llm/context.rs:1-246` |
| `AmplifyResult`, `AtomizeResult`, `DistillResult` | LLM result domain | Typed outputs from task-specific prompts | Used to stage drafts, not directly applied on arrival | `src/llm/context.rs:246-329` |
| `LlmClient`, `ProbeStreamEvent` | service type + stream event | Transport to configured providers | Stateless aside from config and internal `reqwest::Client` | `src/llm/client.rs:23-44` |

### CLI types

| Name | Kind | Purpose | Key invariants / notes | Source |
| --- | --- | --- | --- | --- |
| `Cli`, `Commands` | clap structs/enums | Full CLI parse tree | Command families map closely to store domains | `src/cli/commands.rs:58-118` |
| CLI `BlockId` wrapper | newtype | Accepts raw CLI id text and batch comma forms before store resolution | Parsing is intentionally permissive; validation happens later | `src/cli.rs:55-85` |
| `MountFormatCli`, `BlockPanelBarStateCli`, `OutputFormat` | CLI adapter types | Parse human input into store/domain enums | `FromStr` provides explicit diagnostics | `src/cli.rs:88-154` |

## Function Contracts

### `BloomingBlockery::run_cli`

- Signature: `pub fn run_cli(binary_name: &str) -> anyhow::Result<()>`
- Purpose: Parse CLI input, load the shared store, dispatch one command, persist on success, and print output.
- Requires: A resolvable store path either from `--store` or `AppPaths::data_file`.
- Ensures: Completion generation bypasses store loading; non-error commands save the resulting store.
- Panics: none in normal flow.
- Source: `src/lib.rs:55-95`.

### `BloomingBlockery::gui`

- Signature: `pub fn gui() -> anyhow::Result<()>`
- Purpose: Launch the iced GUI with configured fonts, icon, theme resolver, and subscription pipeline.
- Requires: bundled font/icon assets to be present at compile time.
- Ensures: app boot delegates to `AppState::load`, `AppState::update`, and `AppState::view`.
- Source: `src/lib.rs:120-123` and the surrounding `gui` function.

### `AppState::load`

- Signature: `pub fn load() -> Self`
- Purpose: Build startup state from providers, app config, persisted store, editor buffers, and system appearance.
- Requires: none; missing files degrade to defaults or recovery mode.
- Ensures: provider config validation errors become UI errors; failed store loads enter `persistence_blocked` recovery.
- Gotchas: persistence safety is part of startup, not a later background task.
- Source: `src/app.rs:497-541`.

### `AppState::update`

- Signature: `pub fn update(&mut self, message: Message) -> Task<Message>`
- Purpose: Central mutation router for every GUI event.
- Requires: all feature modules preserve `AppState` invariants.
- Ensures: overlay confirmation state is cleared unless the active message intentionally preserves it.
- Gotchas: some messages are no-ops outside their corresponding mode (`MultiselectBackspace`, link navigation, etc.).
- Source: `src/app.rs:237-357`.

### `AppState::subscription`

- Signature: `pub fn subscription(_state: &AppState) -> Subscription<Message>`
- Purpose: Register global keyboard, mouse, resize, cursor, and system-theme event listeners.
- Requires: none.
- Ensures: emits messages for find/undo/redo/action shortcuts and movement shortcuts, while leaving add-child/add-sibling enter chords to editor-local binding.
- Source: `src/app.rs:372-494`.

### `BlockStore::append_child` / `append_sibling` / `insert_parent`

- Signature family: `pub fn ...(&mut self, ..., point: String) -> Option<BlockId>`
- Purpose: Create new blocks at specific structural positions.
- Requires: anchor block exists; child insertion additionally requires a non-mount parent.
- Ensures: new blocks inherit mount ownership from the anchor when needed.
- Source: `src/store/tree.rs:10-96`.

### `BlockStore::remove_block_subtree`

- Signature: `pub fn remove_block_subtree(&mut self, block_id: &BlockId) -> Option<Vec<BlockId>>`
- Purpose: Delete a subtree and all metadata tied to it.
- Requires: target block exists.
- Ensures: cleans drafts, folds, friend relations, panel state, mount origins; recreates a blank root if the store becomes empty.
- Source: `src/store/tree.rs:122-167`.

### `BlockStore::move_block`

- Signature: `pub fn move_block(&mut self, source_id: &BlockId, target_id: &BlockId, dir: Direction) -> Option<()>`
- Purpose: Reposition a subtree relative to another block.
- Requires: source and target both exist, differ from each other, and do not create an ancestor cycle; `Under` requires a children node.
- Ensures: subtree structure is preserved while location changes.
- Source: `src/store/tree.rs:169-233`.

### `BlockStore::block_context_for_id_with_friend_blocks`

- Signature: `pub fn block_context_for_id_with_friend_blocks(&self, target: &BlockId, friend_block_ids: &[FriendBlock]) -> llm::BlockContext`
- Purpose: Build the full LLM-readable context envelope for one target block.
- Requires: target should exist for useful output.
- Ensures: context includes lineage, direct children, and optional friend lineage / child telescopes.
- Source: `src/store/navigate.rs:94-138`.

### `BlockStore::expand_mount`

- Signature: `pub fn expand_mount(&mut self, mount_point: &BlockId, base_dir: &Path) -> Result<Vec<BlockId>, MountError>`
- Purpose: Load an external subtree file into the current store with fresh ids.
- Requires: target is a `BlockNode::Mount`.
- Ensures: mount table records canonical path, relative path, format, root ids, and all imported ids; mount node becomes `Children`.
- Source: `src/store/mount.rs:228-284`.

### `BlockStore::save` / `save_mounts`

- Signature: `pub fn save(&self) -> io::Result<()>`, `pub fn save_mounts(&self) -> io::Result<()>`
- Purpose: Persist the main store snapshot and expanded mount files.
- Requires: filesystem access to the resolved app directories or mount target paths.
- Ensures: main save excludes mounted descendants and restores expanded mounts to link form in the snapshot.
- Source: `src/store/persist.rs:40-84`.

### `LlmClient::{amplify_block, atomize_block, distill_block, probe_stream}`

- Purpose: Execute task-specific prompts against the configured provider.
- Requires: non-empty `BlockContext`; `probe_stream` also requires non-empty instruction.
- Ensures: structured tasks parse JSON payloads into typed results; `probe_stream` always emits exactly one terminal `Finished` event.
- Gotchas: distill retries plain-text fallback when structured parse fails; probe streaming falls back to one-shot when needed.
- Source: `src/llm/client.rs:44-239`, `src/llm/client.rs:242-321`.

### `Commands::execute`

- Signature: `pub fn execute(self, store: BlockStore, base_dir: &Path) -> (BlockStore, CliResult)`
- Purpose: Dispatch one parsed CLI command into the appropriate command family.
- Requires: caller to provide the current store and base directory for relative mount resolution.
- Ensures: `GenerateCompletion` never reaches this function.
- Source: `src/cli/execute.rs:16-40`.

## Trait Architecture

### Custom traits

- Custom traits: not found.

The codebase prefers concrete structs, enums, free functions, and closures over custom trait hierarchies. The main abstraction mechanisms are data modeling and module boundaries rather than trait objects or pluggable interfaces.

### Manual standard trait implementations

- `Display` for `store::BlockId`, CLI adapters, `ApiStyle`, `MountFormat`, `MaxTokens`. Sources: `src/store.rs:121-126`, `src/cli.rs:79-132`, `src/llm/config.rs:72-75`, `src/store/mount.rs:171-176`, `src/app/config.rs:174-177`.
- `FromStr` for CLI wrappers. Source: `src/cli.rs:61-132`.
- Custom `Serialize` / `Deserialize` for `PointContent` to preserve backward compatibility with older string-only stores. Source: `src/store/point.rs:180-216`.

### Trait-object usage

- `dyn Trait`: not found in core architecture.
- `impl Trait` return positions are used for iterators and streams, especially in LLM streaming and navigation helpers. Sources: `src/app/navigation.rs:169-175`, `src/llm/client.rs:260-264`.

## Error Handling

### Strategy

- Library/domain layers use typed `thiserror` enums (`StoreLoadError`, `MountError`, `LlmError`, config errors).
- Binary/runtime boundaries use `anyhow::Result<()>` only at `main` / startup entry points.
- GUI handlers usually log errors with `tracing` and surface them as `AppError` / `UiError` banners rather than panic.
- CLI execution usually returns `CliResult::Error` and preserves continue-on-error behavior for batch operations.

Sources: `src/store/persist.rs:13-21`, `src/store/mount.rs:44-62`, `src/llm/error.rs:1-120`, `src/app/error.rs:1-97`, `src/lib.rs:55-123`, `src/cli/execute.rs:97-108`.

### Panic policy

- Production-path panics are rare and mostly reserved for internal invariants around widget plumbing or impossible states.
- `expect` / `unwrap` in production code are limited and usually justified by local invariants:
  - `editor_content.expect("editor_content must be Some for text blocks")` assumes non-link rows always have editor buffers. Source: `src/component/point_text_editor.rs:149-153`.
  - `instruction.expect("inquire requires instruction")` is bounded by prompt-builder task semantics. Source: `src/llm/prompt.rs:188-196`.
  - serialization `expect`s in config/provider save paths assume the corresponding structs are always serializable. Sources: `src/llm/config.rs:464-489`, `src/app/config.rs:343-403`.
- Most remaining `unwrap` / `expect` uses are in tests. Source pattern: `rg -n "unwrap\\(|expect\\(" src`.

## Concurrency & Async

| Mechanism | Where used | Why |
| --- | --- | --- |
| `Task::perform` | patch requests, debounced find refresh, file dialogs | One-shot async work that resolves back into one `Message` | `src/app/patch.rs:163-268`, `src/app/find_panel.rs:198-204`, `src/app/mount_file.rs:87-218` |
| `Task::run` + `Task::abortable` | probe streaming in instruction panel | Long-running streamed response that can be cancelled per block | `src/app/instruction_panel.rs:147-181` |
| `Subscription::batch` | global app subscription | Merge keyboard/window/mouse/theme signals into one message stream | `src/app.rs:372-483` |
| `iced::stream::channel` | `LlmClient::probe_stream` | Bridge async SSE / fallback logic into an iced stream of `ProbeStreamEvent`s | `src/llm/client.rs:260-321` |
| `tokio::time::timeout` / `sleep` | patch requests, probe timeout, find debounce | Bound LLM latency and debounce expensive search refreshes | `src/app/patch.rs:166-176`, `src/llm/client.rs:312-315`, `src/app/find_panel.rs:198-204` |
| `Arc<Mutex<...>>` / `Arc<RwLock<...>>` | not found | not needed by current architecture |
| channels outside iced stream | not found | not needed by current architecture |
| `unsafe` | not found | not used |

## Rust Patterns

### Ownership and data modeling

- `slotmap::{SlotMap, SecondaryMap, SparseSecondaryMap}` is the core store primitive. It keeps structural ids stable across mutations while allowing sparse metadata maps keyed by `BlockId`. Sources: `src/store.rs:78-86`, `src/store.rs:217-260`.
- `LazyLock<Option<ProjectDirs>>` caches app path resolution once per process. Source: `src/paths.rs:6-9`.
- The project prefers many small enums/structs over stringly-typed state, especially in LLM config, store point content, and app modes. Sources: `src/store/point.rs:32-170`, `src/llm/config.rs:40-260`, `src/app.rs:810-878`.

### Clone usage

- Intentional high-value clones:
  - Snapshotting `store` and `navigation` for undo. Source: `src/app.rs:787-799`, `src/app.rs:1045-1061`.
  - Copying draft payloads during mount projection / re-keying so mounted stores preserve metadata. Source: `src/store/mount.rs:668-807`.
  - Cloning query/provider/model strings in CLI and settings code because clap and iced callbacks own their input values. Source: `src/cli/execute.rs:49-95`, `src/app/settings.rs:128-140`, `src/app/settings.rs:474-767`.
- ⚠️ Rewrite note: large-store undo currently clones the full `BlockStore`; if document size grows significantly, this will become a scaling pressure point.

### Macros

| Macro | Source | Purpose | Notes |
| --- | --- | --- | --- |
| `rust_i18n::i18n!` | `rust-i18n` | Register locale bundle at crate startup | `src/lib.rs:1-27` |
| `t!` | `rust-i18n` | Resolve UI text in views and handlers | pervasive in `src/app/*` |
| `slotmap::new_key_type!` | `slotmap` | Define `BlockId` key type | `src/store.rs:117-119` |
| `row!`, `column!`, `container`, `scrollable` | `iced` | Declarative widget tree construction | heavily used in `src/app/document.rs`, `src/app/settings.rs`, overlay modules |
| clap derive macros | `clap` | Declarative CLI surface | `src/cli/commands.rs`, `src/cli/tree.rs`, `src/cli/mount.rs`, etc. |

### Build system and features

- Crate type: single package with default GUI binary and extra `bb` binary. Source: `Cargo.toml:1-73`.
- Feature flags: only `log`, enabled by default, gates `tracing-subscriber`. Source: `Cargo.toml:31-33`.
- iced version / features: `0.14` with `tokio` and `image`. Source: `Cargo.toml:37`.
- Notable non-obvious dependencies:
  - `slotmap`: stable ids plus sparse metadata maps.
  - `lucide-icons`: icon font integrated into iced.
  - `similar`: word diff rendering for patch review.
  - `rfd`: async native file dialogs.
  - `dark-light`: system theme detection.
  - `jieba-rs`, `unicode-segmentation`, `regex`: text tokenization and phrase extraction for search / cursor movement.

## iced Patterns

### Elm architecture wiring

- Root app: `iced::application(AppState::load, AppState::update, AppState::view)`. Source: `src/lib.rs:107-123`.
- One root state struct, one root message enum, one subscription function. Sources: `src/app.rs:121-235`, `src/app.rs:372-483`.
- Feature modules own their own nested message enums and `handle` functions rather than implementing mini-applications. Examples: `src/app/find_panel.rs:114-214`, `src/app/mount_file.rs:20-220`, `src/app/instruction_panel.rs:58-180`.

### Layout patterns

- Main document layout uses `column!` for the page shell, `scrollable` for the tree canvas, and overlay layering for find/link/help/error UI. Source: `src/app/document.rs:118-235`.
- Row rendering is componentized around `PointTextEditor`, `IconButton`, `TextButton`, and action-bar view models instead of custom widgets. Sources: `src/component/icon_button.rs:1-98`, `src/component/point_text_editor.rs:43-220`, `src/app/action_bar.rs:158-260`.
- Responsive behavior is mostly width-based: `theme::canvas_max_width` and viewport bucketing for action visibility. Sources: `src/theme.rs:99-134`, `src/app/action_bar.rs:257-260`.

### Custom widgets and canvas

- `Canvas` programs: not found.
- Custom iced `StyleSheet` / `Catalog` implementations: not found.
- Custom widgets in the iced-trait sense: not found.
- The project instead uses small wrapper structs returning preconfigured standard widgets (`IconButton`, `TextButton`, `PointTextEditor`). Sources: `src/component/icon_button.rs:12-98`, `src/component/point_text_editor.rs:43-220`.

### Theming and styling

- Custom theme type: not found. The app uses `iced::Theme` with module-local style functions and a custom semantic `Palette`. Source: `src/theme.rs:1-95`.
- Font usage:
  - Default text font: `LXGW WenKai`.
  - Secondary / UI label font: `Inter`.
  - Icon font: `lucide_icons::LUCIDE_FONT_BYTES`.
  Sources: `src/theme.rs:15-17`, `src/lib.rs:107-118`.
- Palette summary:
  - Light `paper` `#F6F4EF`, `ink` `#2E2B29`, `accent` `#597A9E`, `accent_muted` `#8C9EB3`, `tint` `#EEECE7`, `spine` `#A6A199`, `spine_light` `#C7C2BA`, `danger` `#BF4738`, `success` `#4D9961`, `warning` `#D9A633`.
  - Dark `paper` `#1C1C1F`, `ink` `#D9D4CC`, `accent` `#80A6D1`, `accent_muted` `#738094`, `tint` `#262629`, `spine` `#615E59`, `spine_light` `#403D3B`, `danger` `#D96152`, `success` `#66B87A`, `warning` `#E6B84D`.
  Source-of-truth floats: `src/theme.rs:51-79`.
- Layout token strategy: almost every numeric spacing/sizing constant lives in `theme.rs`; feature modules are explicitly told not to hardcode magic numbers. Sources: `src/theme.rs:97-220`, module docs throughout `src/app/*`.

### iced workarounds and non-obvious choices

- Editor-local vs global shortcut split is a deliberate workaround for overlapping iced key dispatch paths. Sources: `src/app.rs:485-494`, `src/component/point_text_editor.rs:170-220`.
- Probe streaming is implemented with `iced::stream::channel` rather than a separate actor/runtime abstraction. Source: `src/llm/client.rs:260-321`.
- Link previews read files synchronously in view code for markdown/image expansion. ⚠️ This is simple but could become a UI-stall risk for large files or slow filesystems. Source: `src/component/point_text_editor.rs:123-145`.

## Testing Strategy

- Test style: mostly unit-style tests colocated in modules, plus CLI integration tests and store tests. Sources: `src/app.rs:1066-1117`, `src/store.rs:568`, `src/cli.rs:172-175`.
- Critical coverage areas:
  - store structure and mount behavior,
  - app shortcut, patch, instruction, settings, and find behavior,
  - LLM prompt/client parsing,
  - CLI command execution and batch behavior.
- Verification observed during this investigation: `cargo test -q` passed with `478` tests.
