# Async Conflict Safety

How bb prevents stale LLM responses from overwriting newer edits.

## Problem

Expand/reduce requests are asynchronous. A user can edit the document while a request is in flight. Without a guard, an old response could be applied to a newer document state.

## Approach

1. When `Reduce` or `Expand` starts, the app computes a **request signature** from the full lineage (root-to-target points) used to build the LLM prompt.
2. The signature is stored in `pending_reduce_signatures` or `pending_expand_signatures` for that block.
3. When `ReduceDone` or `ExpandDone` arrives, the app recomputes the current lineage signature for the same block.
4. If signatures differ (or the block no longer exists), the response is treated as stale and ignored.
5. If signatures match, the response is applied normally (draft creation or error handling for real request failures).

## Why lineage, not only target text

The LLM prompt includes lineage context, not just the target point. If an ancestor point changes, the prompt meaning changes. Hashing the full lineage ensures stale detection matches what was actually sent.

## State and lifecycle

- Request tracking maps live in `AppState` and are cleared on snapshot restore.
- Archive/removal clears pending signatures and async states for removed blocks.
- Late responses for missing blocks are ignored.

## User-visible behavior

- Real request failures still produce error state.
- Stale responses do not show as errors; they are dropped quietly.
