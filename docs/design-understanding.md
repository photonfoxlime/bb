# Design Understanding

Context document for AI assistants. Records design decisions and current implementation state so future sessions stay aligned.

For project purpose, data model, and workflow, see `README.md` (canonical source).

## Product Principle

Prioritize clarity of thought structure over UI chrome.
Help users think in branches, compare alternatives, and progressively refine ideas.
Preserve tree readability first. Avoid timeline metaphors. Keep structural-spine concept.

## Aesthetic Direction

- Light, airy, calm. Soft blue-ink tone. Paper-like background. Generous whitespace.
- Structure visible through vertical spines and uniform dot markers, not decorative widgets.
- Actions feel like marginalia annotations, not toolbar chrome.
- Fonts: LXGW WenKai for point text (default), Inter for utility labels and buttons.
- Palette defined in `src/theme.rs`: PAPER (warm off-white), INK (near-black), ACCENT (soft blue), ACCENT_MUTED, TINT (warm gray), SPINE (low-contrast gray), DANGER, SUCCESS.

## Architecture

### Module structure

- `src/main.rs` -- Iced app entry. Loads fonts (WenKai, Inter, Lucide), wires theme.
- `src/app.rs` -- State, messages, update, view. Contains `BlockGraph`, `EditorStore`, `TreeView`, `ExpansionDraft`, lifecycle states.
- `src/app/action_bar/` -- Typed action bar: types, selector (state-to-VM), responsive projection, keyboard shortcuts, dispatch.
- `src/llm.rs` -- LLM client, config loading (env vars + TOML file), prompt construction, expand/summarize API.
- `src/theme.rs` -- Custom paper-and-ink theme: palette constants, per-widget style functions.

### Key types

| Type | Location | Purpose |
|------|----------|---------|
| `BlockId` | `app.rs` | UUID wrapper for stable block identity |
| `BlockGraph` | `app.rs` | Roots + HashMap\<BlockId, BlockNode\>. JSON serialization. |
| `BlockNode` | `app.rs` | Point (String) + children (Vec\<BlockId\>) |
| `EditorStore` | `app.rs` | Maps BlockId to iced text_editor::Content buffers |
| `AppState` | `app.rs` | Full UI state: graph, editors, LLM config, expand/summary lifecycle, drafts |
| `TreeView` | `app.rs` | Pure renderer: borrows immutable AppState, produces Element tree |
| `ExpansionDraft` | `app.rs` | Pending expand result: optional rewrite + child suggestions |
| `LlmClient` | `llm.rs` | HTTP client for summarize and expand requests |
| `LlmConfig` | `llm.rs` | base_url, api_key, model. Loaded from env vars or `llm.toml` |

### Design decisions

- **UUID-based addressing** (not structural paths). Block identity is stable across tree mutations.
- **Lineage-based context**. Summarize and expand use DFS root-to-target lineage as LLM context.
- **Single-block async lifecycle**. One summarize and one expand operation active at a time (`SummaryState` / `ExpandState` enums).
- **Draft-then-apply**. Expand results land in `ExpansionDraft` for review. Rewrite and each child accepted/rejected independently.
- **Pure renderer**. `TreeView` borrows immutable state, produces widgets. No mutation during rendering.

### Iced layout pitfall (keep in mind)

Iced's `center_x(width)` overrides the preceding `width(...)` call because it is implemented as `self.width(width).align_x(Center)`. Use `.align_x(Horizontal::Center)` instead to preserve explicit widths on containers.

## Action Bar

See [`action-bar.md`](action-bar.md) for full action bar design: structure, responsive projection, keyboard shortcuts, state rules, and visual details.

## Visual Implementation (Current Values)

- Spine: `rule::vertical(1)` with `spine_rule` style, 4px container.
- Marker: bullet character size 12, 12px container, top-padded 3px for baseline alignment.
- Content column: `max_width(720)`, centered.
- Child indent: `Padding::ZERO.left(16.0)`.

## Exploration Backlog

### Interaction
- Undo/redo with typed command events.
- Collapse/expand subtree visibility.
- Keyboard-first traversal (up/down, indent-level moves).
- Conflict-safe editing during async operations.

### Expand/Summarize quality
- Retry and fallback parsing for expand JSON.
- Prompt tuning for concise, non-overlapping suggestions.
- Per-request cancellation and timeout in UI.

### Data model
- Versioned storage schema for migrations.
- Graph integrity checks (dangling ids, cycles).
- Typed metadata on blocks (references, open-as-document flag).
- Import/export format.

### Polish
- Style tokens in one theme module (spacing, type ramp, radii).
- Subtle motion for expand/collapse and suggestion reveal.
- Narrow-width/mobile tuning.

### Safety
- Confirmation for destructive operations.
- Autosave feedback and recovery indicator.
- Operation log for AI-generated changes.

### Testing
- Graph mutation and lineage resolution unit tests.
- Expand draft state transition tests.
- Serialization round-trip tests.
- Save/load integration tests with malformed data.
