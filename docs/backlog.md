# Exploration Backlog

Unimplemented ideas and future directions.

For current architecture, see [architecture.md](architecture.md).

## Active Priorities

### Data model
- Versioned storage schema for migrations.
- Graph integrity checks (dangling ids, structural validation).
- Typed block metadata (references, open-as-document flag).
- Import/export format.

### UX polish
- Uniform bullet sizing (root bullet is visually heavier than child bullets).
- More horizontal breathing room between editor and action icons.
- Minimum editor width for short text rows.
- Draft panel typographic hierarchy for section labels.
- Empty-state placeholder for blank document.
- Subtle motion for expand/collapse and suggestion reveal.
- Narrow-width/mobile tuning.

### Safety and observability
- Confirmation for destructive operations.
- Autosave/recovery indicator.
- Operation log for AI-generated changes.

### Testing
- Graph mutation and lineage resolution unit tests.
- Serialization round-trip tests.

## Recently Completed (kept for context)

- Collapse/expand subtree visibility.
- Keyboard-first traversal across visible DFS order.
- Stale async response guard via lineage signatures.
- Prompt tuning for concise, non-overlapping expansion suggestions.
- Per-request cancellation and timeout for expand/reduce.
- Centralized style tokens in theme module.
- Expand/reduce state transition tests.
- Strict malformed JSON load behavior and save normalization path coverage.
- Mounted persistence regression coverage (deep nodes, siblings, duplicates, nested save-back).
