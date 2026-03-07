# UI Component Extraction Plan

Analysis of duplicated patterns across `src/app/` UI modules,
with concrete extraction opportunities ordered by impact.

## 1. Floating Search Panel (highest duplication)

**Files:** `link_panel.rs` (679 LOC), `find_panel.rs` (650 LOC), `archive_panel.rs` (136 LOC)

All three share ~80% of their view structure:

- Mode-based visibility check; return invisible spacer when inactive
- Title bar with close button
- Query `text_input` with auto-focus
- Scrollable list of candidate rows (button-styled, full-width)
- Wrapping via `floating_panel::wrap(content, width, height)`

**What varies:** item content rendering, message types, query/selection logic.

**Extraction:** A generic `FloatingSearchPanel<T>` that accepts item data + a render
closure, provides the standard shell (visibility, title, close, scroll, wrap).
Estimated savings: ~400 LOC.

## 2. Search State Management

**Files:** `link_panel.rs`, `find_panel.rs`

Both independently implement:

- Query text + candidate list (`Vec<T>`)
- Selected index with up/down navigation
- Debounce revision tracking for query refresh

**Extraction:** `SearchState<T>` struct with `query`, `candidates`, `selected`,
`next()`/`prev()` methods. Estimated savings: ~80 LOC.

## 3. Inline Editor Widget

**File:** `friends_panel.rs` (lines 272-364)

Generic pattern: display value -> click edit -> text_input + accept/cancel -> save.
Currently one-off but fully generic.

**Extraction:** `InlineEditor<T>` that accepts an initial value, renders toggle between
display and edit mode, fires callback on confirm. Estimated savings: ~100 LOC;
enables reuse wherever inline editing is needed.

## 4. List Item Row

**Files:** `archive_panel.rs`, `find_panel.rs`, `friends_panel.rs`

Repeated pattern: `row![content.width(Fill), action_buttons]` wrapped in a styled button.

**Extraction:** `ListItemRow` builder -- content on left, actions on right,
consistent hover/padding. Estimated savings: ~80 LOC.

## 5. Draft Panel Container

**Files:** `instruction_panel.rs`, `friends_panel.rs`, `patch_panel.rs`

Identical wrapping:
```rust
container(panel)
    .padding(Padding::from([theme::PANEL_PAD_V, theme::PANEL_PAD_H]))
    .style(theme::draft_panel)
```

Small but easy to unify for visual consistency.

## 6. Panel Toggle Handlers

**Files:** `friends_panel.rs`, `instruction_panel.rs`, `archive_panel.rs`

Nearly identical toggle logic:
```rust
if mode == DocumentMode::X { set Normal } else { set X }
```

Could be a method on `DocumentMode` or a small helper.

## 7. Toolbar Builder (low priority)

**Files:** `document_toolbar.rs` (100 LOC), `document_top_right.rs` (78 LOC)

Both render icon button rows with `ACTION_GAP` spacing. Could share a builder,
but the files are small so the win is mostly consistency.

## Implementation Phases

| Phase | Extract | Impact |
|-------|---------|--------|
| 1 | `FloatingSearchPanel<T>` + `SearchState<T>` | ~480 LOC saved, 3 files simplified |
| 2 | `InlineEditor` + `ListItemRow` | ~180 LOC saved, enables future reuse |
| 3 | `DraftPanelContainer` + toggle helpers | Consistency, ~40 LOC |
