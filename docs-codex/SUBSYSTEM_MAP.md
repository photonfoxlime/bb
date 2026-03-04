# Blooming Blockery — Rewrite-Ready Subsystem Map

## Top-Level Dependency Shape

```text
BloomingBlockery (src/lib.rs)
├── GUI runtime (src/app.rs)
│   ├── Document rendering + components
│   ├── Interaction handlers + transient UI state
│   ├── Settings/config screen
│   ├── LLM request orchestration
│   └── Theme + i18n + file dialogs
├── CLI runtime (src/cli.rs, src/cli/*)
│   └── Command dispatch over the shared store
├── Shared core model (src/store.rs, src/store/*)
│   ├── Tree structure + point content
│   ├── Drafts + friend metadata
│   ├── Search + lineage + LLM context
│   ├── Persistence + mounts
│   └── Markdown mount format
├── Shared LLM layer (src/llm.rs, src/llm/*)
├── Shared paths/config/text utilities
└── Shared UI helper components + theme
```

Primary sources: `src/lib.rs:55-123`, `src/app.rs:48-76`, `src/cli.rs:34-47`, `src/store.rs:63-69`, `src/llm.rs:12-16`.

## Recommended Rewrite Order

1. `paths` + persistence contracts
2. `store` core types (`BlockId`, `BlockNode`, `PointContent`, `BlockStore`)
3. tree mutations, search, lineage, and LLM context assembly
4. mounts + markdown mount format
5. LLM config, prompt building, and client
6. app shell (`AppState`, `Message`, subscriptions, undo snapshots)
7. editor buffers, point editor component, action bar, shortcuts
8. overlays and panels (find, link, friends, instruction, mount UI)
9. settings screen and app config persistence
10. CLI command families and output formatting

This order matches the real dependency direction in the code: the GUI and CLI both terminate in `BlockStore`, while LLM and config layers are shared services consumed by the GUI and partly by the CLI. Sources: `src/lib.rs:55-123`, `src/app.rs:121-235`, `src/cli/execute.rs:16-40`.

## Subsystems

| Subsystem | Responsibility | Owns State / Contracts | Depends On | Key Files |
| --- | --- | --- | --- | --- |
| Runtime entry points | Select GUI vs CLI runtime, initialize tracing, load assets | `BloomingBlockery` | `app`, `cli`, `paths`, `store`, `theme` | `src/lib.rs:49-123`, `src/main.rs:1-6`, `src/bin/bb.rs:1-8` |
| Core block store | Forest structure, point content, per-block metadata, mount ownership | `BlockStore`, `BlockNode`, `BlockId`, draft maps | `slotmap`, `serde`, `paths` | `src/store.rs:1-260`, `src/store/drafts.rs:1-260`, `src/store/tree.rs:1-320`, `src/store/navigate.rs:1-230` |
| Persistence and paths | Resolve app directories, load/save main store, save mounted stores | `AppPaths`, `StoreLoadError` | `directories`, filesystem | `src/paths.rs:1-40`, `src/store/persist.rs:1-84` |
| Mount subsystem | External subtree projection, re-keying, move/inline/collapse, markdown mount support | `MountTable`, `MountEntry`, `MountError`, `MountFormat` | `BlockStore`, filesystem, serde | `src/store/mount.rs:1-240`, `src/store/markdown.rs:1-162` |
| LLM domain | Provider config, prompt contracts, context/result types, HTTP client | `LlmProviders`, `TaskKind`, `BlockContext`, `LlmClient` | `reqwest`, `tokio`, `serde`, `paths` | `src/llm/config.rs:1-260`, `src/llm/context.rs:1-260`, `src/llm/client.rs:44-420`, `src/llm/prompt.rs:1-120` |
| App shell | Elm root state, message router, subscriptions, persistence safety, undo snapshots | `AppState`, `Message`, `TransientUiState`, `DocumentMode`, `ViewMode` | `store`, `llm`, `undo`, `iced` | `src/app.rs:121-235`, `src/app.rs:237-541`, `src/app.rs:879-1064` |
| Document renderer | Pure view over the current state; tree layout, toolbar, overlays, breadcrumbs | `DocumentView`, `TreeView` | `theme`, components, app submodules | `src/app/document.rs:118-260`, plus the rest of `src/app/document.rs` |
| Interaction handlers | Focused state mutations for edit, structure, shortcuts, overlays, navigation | module-local `*Message` enums | `AppState`, `BlockStore`, `EditorBuffers` | `src/app/edit.rs`, `src/app/structure.rs`, `src/app/shortcut.rs:1-260`, `src/app/navigation.rs:1-220`, `src/app/overlay.rs:1-59` |
| Panels and overlays | Find, link mode, friends, instruction, context menu, error banner, patch panel | `FindUiState`, `LinkPanelState`, panel-local messages | `AppState`, `llm`, `store`, `theme` | `src/app/find_panel.rs:1-220`, `src/app/link_panel.rs:1-220`, `src/app/friends_panel.rs:1-215`, `src/app/instruction_panel.rs:1-180`, `src/app/patch.rs:1-180` |
| Settings and config | Provider CRUD, task model/prompt settings, locale/theme prefs, path display | `SettingsState`, `AppConfig`, `TaskConfig`, `MaxTokens` | `llm`, `paths`, `theme` | `src/app/settings.rs:1-220`, `src/app/config.rs:1-220` |
| UI primitives and theme | Shared icon/text button builders, point editor widget, style tokens, palette | `IconButton`, `TextButton`, `PointTextEditor`, `Palette` | `iced`, `lucide-icons` | `src/component/icon_button.rs:1-98`, `src/component/point_text_editor.rs:1-220`, `src/theme.rs:1-220` |
| CLI surface | Command parsing, ID resolution, batch expansion, output formatting | `Cli`, `Commands`, command family enums, result types | `clap`, `store`, `llm::context` | `src/cli.rs:1-171`, `src/cli/commands.rs:1-118`, `src/cli/execute.rs:16-141` |

## Rewrite Notes Per Subsystem

### Runtime entry points

- Recreate these last only if you are building a binary-compatible distribution; everything else can be developed and tested underneath the library boundary first.
- The public crate surface is intentionally small. Most modules are private to the crate; `BloomingBlockery` is the explicit runtime namespace. Sources: `src/lib.rs:29-38`, `src/lib.rs:49-123`.

### Core block store

- Treat this as the canonical domain model. If the rewrite changes UI architecture but preserves functionality, this layer should still be rewritten first and tested heavily.
- The split between `nodes` and `points` is not incidental; it makes structural mutations and typed point evolution cheaper than storing whole blocks inline. Sources: `src/store.rs:158-260`.

### Mount subsystem

- This is the hardest non-LLM subsystem to reproduce correctly because it combines persistence, identity remapping, and partial projection back into files.
- Rewrite its tests early, not late. Many correctness claims here are already captured in `src/store/tests.rs`. Sources: `src/store/mount.rs:228-284`, `src/store/persist.rs:40-84`.

### App shell

- `AppState` deliberately separates durable state from transient UI state and from widget buffers. Do not collapse those three categories in a rewrite unless you want to re-open persistence and undo bugs.
- The global subscription layer is also a design boundary: some shortcuts are intentionally global, others intentionally editor-local. Sources: `src/app.rs:204-235`, `src/app.rs:372-494`, `src/component/point_text_editor.rs:170-220`.

### Panels and overlays

- These modules are decoupled enough to be rewritten incrementally once `AppState`, `Message`, and `BlockStore` are stable.
- The probe panel and patch panel are the only places where async LLM work materially changes user-visible draft state. Sources: `src/app/patch.rs:101-180`, `src/app/instruction_panel.rs:123-180`.

### CLI surface

- The CLI is mostly a declarative shell over the store. It should be rewritten after the store and persistence formats are stable.
- Batch expansion and pair broadcasting are reusable execution helpers, not command-specific hacks. Sources: `src/cli/execute.rs:49-95`.

## Rewrite Risks

- Mount correctness: highest risk because of re-keying, nested mounts, and projection semantics.
- Async LLM staleness: high risk because stale responses must not overwrite later edits.
- Shortcut routing: medium-high risk because editor-local and global shortcut paths intentionally overlap only in specific cases.
- Persistence recovery mode: medium risk because data-loss protection depends on preserving the `persistence_blocked` semantics.

Sources: `src/store/mount.rs:228-284`, `src/app/patch.rs:101-180`, `src/app/instruction_panel.rs:123-180`, `src/app.rs:497-541`.
