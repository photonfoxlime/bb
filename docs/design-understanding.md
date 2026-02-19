# Design Understanding Log

This document records my current understanding of the project and design direction for this session.
It is written for future iterations of the assistant so context is preserved and implementation choices stay aligned with intent.

## Project Purpose

`bb` is a document editor for text-first design and implementation.
It helps users iteratively shape ideas by combining manual editing with LLM-assisted expansion and reduction.

## Core Data Model

The document is a tree of blocks.

Each block contains:
- an optional terminal text element (`point`),
- a list of child trees (`forest`),
- additional attributes (for example named references or flags like "open as document").

The root is itself a block.
Higher-order structure is represented naturally via nested sub-blocks.

## Intended User Workflow

1. User writes a short initial prompt as a block point.
2. User triggers **expand** on that block.
3. LLM returns:
   - possible rewrite(s) of the current point,
   - concise child sub-block suggestions (single readable points).
4. User keeps useful sub-blocks and discards weak ones.
5. User develops one selected sub-block (iterative deepening).
6. User may also re-expand an existing block for additional inspiration.
7. User may use **reduce** to compress verbose points into concise ones.

## UI Design Understanding (Current)

The interface should feel calm and handwritten, with structure made visually obvious and controls kept lightweight.

### Visual Structure

- Tree rendered as vertical structural spines.
- Every block uses the same simple dot marker on the spine.
- Block text appears to the right of the marker.
- Nested levels create additional aligned spine columns.
- Spines represent parent/child hierarchy, not time.

### Interaction Tone

- Inline actions (for example expand and reduce) should feel like annotations, not heavy toolbar chrome.
- Keep editing flow lightweight so idea structure remains primary.

### Aesthetic Direction

- Light, airy look.
- Soft blue-ink tone.
- Paper-like background texture.
- Generous whitespace.
- Strong legibility and reading flow.

## Product Principle to Preserve

Prioritize clarity of thought structure over UI chrome.
The UI should help users think in branches, compare alternatives, and progressively refine ideas.

## Working Notes for This Session

- Preserve tree readability first; avoid visually noisy controls.
- Keep generated sub-points concise and scannable.
- Avoid introducing timeline metaphors in layout.
- Any detailed design should remain faithful to the handwritten, structural-spine concept.

## UI Planning v1 (Element and Operation First)

This section outlines the implementation plan before detailed visual polish.
It starts from what is rendered and what users can do to each element.

### Element Inventory

- Document canvas (scrollable paper area).
- Spine column per depth level.
- Block row (dot marker, editable point text, inline actions).
- Child branch region (indented nested blocks).
- Inline result states (loading, error, suggestion list).
- Global status strip for non-local errors.

### Operations per Element

#### Block row

- Edit point text directly.
- Expand point into child suggestions.
- Reduce point to concise summary.
- Accept or reject each generated child suggestion.
- Add a manual child point.
- Delete or archive a block.

#### Child branch region

- Collapse or expand branch visibility.
- Reorder children.
- Open one child as current working focus.

#### Document canvas

- Scroll and navigate hierarchy.
- Search and jump to matching points.
- Keyboard-first traversal (up, down, indent-level moves).

### UX Principles for Implementation

- Keep one primary action in focus per row; reveal secondary actions on hover/focus.
- Preserve text stability during async operations (no layout jumps).
- Keep branch context visible while editing a child node.
- Prefer reversible actions and soft-delete where possible.
- Ensure complete keyboard accessibility for writing-heavy sessions.

### Visual System Plan

- Use WenKai for point text and Inter for utility labels/buttons.
- Define a restrained ink palette: paper, ink, muted accent, warning, error.
- Render subtle paper texture and low-contrast structural lines.
- Keep markers and spines uniform across depths.
- Use spacing rhythm to communicate hierarchy more than decorative widgets.

### Motion and Feedback Plan

- Animate expand and collapse with short height and opacity transitions.
- Use staggered reveal for generated child suggestions.
- Show local progress at the row level during expand/reduce.
- Keep motion calm and low amplitude to preserve reading comfort.

### Incremental Delivery Plan

1. Build structural layout primitives (canvas, spine, block row, child region).
2. Add interaction model (edit, expand/reduce triggers, async states).
3. Add suggestion acceptance workflow and branch operations.
4. Add keyboard navigation and accessibility polish.
5. Apply visual theming and motion tuning.
6. Validate desktop and narrow-width behavior, then adjust spacing and controls.

### Current Code Reality (for Alignment)

- App supports inline point editing, summarize, and expand actions per block.
- Block rows render lightweight spine and dot markers.
- Expand produces a per-block draft panel with optional rewrite plus child suggestions.

## Session Progress Notes

### Implemented: Functional state and render structure

- Added typed identifier-based addressing using `BlockId` (UUID) for all row operations.
- Added `EditorStore` as a dedicated state container for editor buffers.
- Added typed UI error models (`UiError`, `AppError`) for banner and row state.
- Added typed summarize lifecycle with per-block-id states (`Idle`, `Loading`, `Error`).
- Added `TreeView` renderer struct to keep view generation pure from immutable state.
- Added lightweight spine and dot markers in block rows to align with the tree visual concept.
- Added `tracing` logs for edit, summarize start/success/failure, and save failures.

### Why this shape

- UUID-based block addressing keeps operations stable even if structural position changes.
- Renderer struct keeps data-to-view mapping explicit and easier to iterate for design polish.
- Dedicated stores and error enums make async UX behavior predictable and testable.

### Pivot note

- Structural path addressing was intentionally replaced with UUID-based addressing.
- Summary lineage is now resolved by DFS from roots to a target block id.
- This keeps command/event payloads focused on persistent identity rather than temporary tree position.

### Implemented: Expand workflow (identifier-based)

- Added `Expand(BlockId)` async flow with typed expand lifecycle state.
- Added LLM expand API contract with strict JSON payload: optional rewrite plus child suggestions.
- Added per-block `ExpansionDraft` state for review before applying generated content.
- Added row-level operations to apply/dismiss rewrite and keep/drop each child suggestion.
- Added bulk action to accept all suggested children in one step.
- Added graph mutation helpers to append children by parent block id.

### Notes for next iteration

- Keep expand panel visually lighter; current bordered panel is functional but not final aesthetic.
- Add explicit undo for applied rewrite and accepted children.
- Add keyboard shortcuts for keep/drop actions to speed up review flow.

## Exploration TODO Backlog

This backlog captures likely next areas to explore after the current identifier-based summarize and expand baseline.

### Interaction and editing

- Add create/delete/archive operations per block (with reversible delete).
- Add block reorder operations for sibling branches.
- Add collapse/expand visibility state per subtree.
- Add keyboard-first traversal and action shortcuts.
- Add quick "new child" and "new sibling" actions for manual writing flow.

### Expand and summarize quality

- Add retry and fallback parsing for expand JSON responses.
- Add prompt tuning for concise, non-overlapping child suggestions.
- Add richer summarize modes (very short, balanced, context-heavy).
- Add per-request cancellation and timeout handling in UI.
- Add conflict-safe behavior when user edits while async response arrives.

### Data model and persistence

- Add versioned storage schema for future migrations.
- Add safe load behavior (error reporting, backup/restore path).
- Add invariant checks for graph integrity (dangling child ids, cycles).
- Add typed metadata fields on blocks (references, open-as-document flag).
- Add import/export format for document sharing.

### UX and visual design polish

- Replace placeholder spine/marker rendering with refined visual primitives.
- Introduce style tokens (palette, spacing, type ramp, radii) in one theme module.
- Improve draft suggestion panel hierarchy and readability.
- Add subtle motion for expand/reduce and suggestion reveal.
- Tune narrow-width/mobile behavior for controls and text editor width.

### Safety, history, and trust

- Add undo/redo history with typed command events.
- Add explicit confirmation for destructive operations.
- Add autosave feedback and recovery indicator.
- Add optional operation log panel for AI-generated changes.

### Observability and diagnostics

- Add tracing spans for full async lifecycle (request -> response -> apply).
- Add structured metrics counters for summarize/expand success and failure.
- Add user-visible diagnostic details for common LLM/config failures.

### Testing and reliability

- Add unit tests for graph mutations and lineage resolution.
- Add tests for expand draft state transitions and accept/reject flows.
- Add serialization round-trip tests for UUID-based ids.
- Add integration tests for save/load behavior under malformed data.

### Documentation

- Document block graph invariants in code-level docs.
- Document interaction model and message/state machine for contributors.
- Add a short architecture note: data flow from UI event to persisted graph.

## Action Bar (Consolidated)

### Intent

- Keep actions lightweight, inline, and secondary to writing.
- Provide clear row-local feedback for loading, error, and draft states.
- Preserve text stability and avoid layout jumps.

### Structure

- Row zones: structure marker, editor, status lane, action bar.
- Primary actions (always visible on desktop): `Expand`, `Reduce`, `Add child`.
- Contextual actions (state-driven): `Accept all`, `Retry`, `Dismiss draft`.
- Overflow actions: branch/focus extras plus `Add sibling`, `Duplicate`, `Archive`.

### Responsive behavior

- `Wide`: primary + contextual inline.
- `Medium`: contextual moves to overflow.
- `Compact`: `Reduce` also moves to overflow.
- `TouchCompact`: menu-first access to keep editor readable.

### Keyboard and dispatch

- Shortcuts:
  - `Ctrl+.` expand
  - `Ctrl+,` reduce
  - `Ctrl+Enter` add child
  - `Ctrl+Shift+Enter` add sibling
  - `Ctrl+Shift+A` accept all
  - `Ctrl+Backspace` archive
- Pointer and keyboard both route through the same `ActionId -> dispatch` path.

### State and feedback rules

- Busy states disable only conflicting actions.
- Error state shows compact chip + retry affordance.
- Draft state surfaces accept/dismiss quickly.
- Empty point gates reduce; add child remains available.

### Implementation mapping

- Types: `ActionId`, `ActionAvailability`, `ActionDescriptor`, `StatusChipVm`, `ActionBarVm`, `RowContext`.
- Core functions:
  - state selector builds action VM from immutable row context,
  - responsive projector demotes actions deterministically,
  - dispatcher maps action id to block-id command/message.

### Current implementation status

- Typed action-bar module exists and is integrated into row rendering.
- Overflow is interactive (toggle + actionable items).
- Shortcuts are wired via global event subscription to the same dispatcher path.
- Auto-close implemented: closes overflow on uncaptured mouse press and on `Escape`.

### Immediate next steps

- Add explicit undo/redo for destructive and AI-apply actions.
- Add collapse/expand-branch and open-as-focus overflow actions.
- Add targeted tests for overflow interaction and shortcut edge cases.
