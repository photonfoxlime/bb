# Exploration Backlog

Unimplemented ideas and future directions. Not committed work -- just captured for reference.

For current architecture, see [architecture.md](architecture.md).

## Interaction
- Collapse/expand subtree visibility.
- Keyboard-first traversal (up/down, indent-level moves).
- Conflict-safe editing during async operations.

## Expand/Summarize quality
- Retry and fallback parsing for expand JSON.
- Prompt tuning for concise, non-overlapping suggestions.
- Per-request cancellation and timeout in UI.

## Data model
- Versioned storage schema for migrations.
- Graph integrity checks (dangling ids, cycles).
- Typed metadata on blocks (references, open-as-document flag).
- Import/export format.

## Polish
- Style tokens in one theme module (spacing, type ramp, radii).
- Subtle motion for expand/collapse and suggestion reveal.
- Narrow-width/mobile tuning.

## Safety
- Confirmation for destructive operations.
- Autosave feedback and recovery indicator.
- Operation log for AI-generated changes.

## Testing
- Graph mutation and lineage resolution unit tests.
- Expand draft state transition tests.
- Serialization round-trip tests.
- Save/load integration tests with malformed data.
