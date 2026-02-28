---
name: blooming-blockery-cli
description: Documents the blooming-blockery CLI contract and teaches context-first tree authoring for design specifications and documentation.
---

# blooming-blockery-cli

Use this skill when operating the `blooming-blockery` CLI and when shaping
content into tree structure for better LLM context quality.

Primary goal: treat Blooming Blockery as a tree-structure based design
documentation editor. Organize information into a logically correct tree so
each block carries one coherent idea, and unrelated material is split into
separate branches.

This skill is not only command syntax reference. It is a structure-and-context
guide for deciding where information should live in the tree.

## Motivation: From Vague Ideas to Implementation

Use the tree to move from uncertainty to precision:

1. Start with broad project ideas or high-level concepts.
2. Break ideas into smaller design decisions and scope boundaries.
3. Refine decisions into explicit specifications, such as `requires`, `ensures`,
   and invariants.
4. Continue refining specification blocks into implementable units.
5. Connect implementation steps back to the exact specification branch they
   satisfy.

Agents using this skill should preserve this top-down refinement flow whenever
possible.

## When to use

- The user asks how to use `blooming-blockery` from terminal scripts.
- The user needs to restructure mixed or overloaded notes into a cleaner tree.
- The user needs LLM-ready context around a specific block.
- The user hits CLI parsing errors with block IDs, mount formats, or panel states.
- The user needs copyable command patterns for repeated authoring workflows.

## Mental Model: Context Envelope Per Block

From `src/llm/context.rs`, one block's LLM context is a `BlockContext` made of:

- `lineage`: ordered chain from root to target, where the last item is the
  target block itself.
- `existing_children`: direct child point texts only.
- `friend_blocks`: cross-references with optional `perspective`, optional friend
  lineage telescope, and optional friend children telescope.

Implications for agents using this skill:

- Keep each block focused. If one block mixes unrelated concerns, split it.
- Prefer children for decomposition of one topic.
- Prefer siblings or new roots for unrelated topics.
- Prefer friends for cross-cutting links that are related but not hierarchical.
- Add `--perspective` when a friend relation is ambiguous.
- Use telescope flags intentionally to expand friend context only when needed.

## Context-Hygiene Rules for Tree Editing

1. One block, one intent.
2. If two statements cannot share the same immediate design question, split them.
3. Keep parent-child logic strict: each child should refine, justify, or execute
   the parent.
4. Keep implementation detail under the relevant design/spec parent.
5. Do not bury unrelated decisions deep in another topic's lineage.
6. Preserve refinement order: concept -> spec (`requires`/`ensures`/invariants)
   -> implementation.
7. Use `block context <BLOCK_ID>` to inspect context before LLM draft commands.
8. Use `block nav lineage <BLOCK_ID>` when checking whether placement is right.
9. Use `block friend add` instead of forced tree nesting for cross-topic links.

## When Tree Structure Is Hard, Use Friend Blocks

Some content is inherently cross-cutting and does not fit cleanly under one
single parent branch. In those cases:

- Keep the main tree logically clean.
- Use `block friend add <TARGET_ID> <FRIEND_ID>` for remote references.
- Add `--perspective` to explain why the remote block matters to the target.
- Enable telescope flags only when extra friend lineage/children are truly
  needed.

Friend blocks are the escape hatch for useful context that should not distort
the core tree semantics.

## CLI Invocation Convention

All examples below are shown with full command names:

```bash
blooming-blockery block ...
```

If you need shorter snippets in explanations, you may omit `blooming-blockery`
and start at `block ...`, but keep full-form examples in instructions and
automation scripts.

## Global Flags

All commands support these global flags:

- `--store <PATH>`: Path to the block store file (defaults to app data path)
- `--verbose`: Enable verbose output (currently reserved)
- `--output <FORMAT>`: Output format - `table` (default) or `json`

## Block ID Format

Block IDs use a clean format like `1v1`, `2v3` where:
- First number = slot index in the store
- `v` = separator
- Second number = generation counter (increments on reuse)

Batch-capable commands also accept comma-separated IDs in a single ID argument:

```bash
blooming-blockery block show 1v1,2v1,3v1
blooming-blockery block tree add-child 1v1,2v1 "Shared child text"
```

Batch execution is continue-on-error: all targets are attempted and errors are
reported after processing completes.

## Context-First Workflow Patterns

### 1) Split unrelated information into separate branches

```bash
# Add a sibling when topic shifts at same hierarchy level
blooming-blockery block tree add-sibling <BLOCK_ID> "New topic"

# Wrap current block when introducing a new organizing parent
blooming-blockery block tree wrap <BLOCK_ID> "Parent concept"

# Move misplaced content under the correct parent
blooming-blockery block tree move <SOURCE_ID> <TARGET_ID> --under
```

### 2) Inspect context before asking LLM features

```bash
blooming-blockery block nav lineage <BLOCK_ID>
blooming-blockery block context <BLOCK_ID> --output json
blooming-blockery block friend list <BLOCK_ID> --output json
```

### 3) Keep cross-cutting references explicit

```bash
blooming-blockery block friend add <TARGET_ID> <FRIEND_ID> \
  --perspective "reference architecture constraint" \
  --telescope-lineage
```

Use `--telescope-lineage` and `--telescope-children` only when the added friend
scope is needed for the specific LLM task.

## Command Reference

### Query Commands

```bash
# List all root block IDs
blooming-blockery block roots
blooming-blockery block roots --output json

# Show block details
blooming-blockery block show <BLOCK_ID>
blooming-blockery block show 1v1 --output json

# Search blocks by text (case-insensitive substring)
blooming-blockery block find "search query"
blooming-blockery block find "TODO" --limit 10

# Edit the text content of a block
blooming-blockery block point <BLOCK_ID> "New text content"
blooming-blockery block point 1v1 "Updated text"
```

### Tree Structure Commands

```bash
# Add child block under parent (parent must not be a mount)
blooming-blockery block tree add-child <PARENT_ID> "Text content"
blooming-blockery block tree add-child 1v1 "My new idea"

# Add sibling after a block
blooming-blockery block tree add-sibling <BLOCK_ID> "Text content"
blooming-blockery block tree add-sibling 1v1 "Next sibling"

# Wrap a block with a new parent
blooming-blockery block tree wrap <BLOCK_ID> "Parent text"
blooming-blockery block tree wrap 1v1 "New parent section"

# Duplicate a subtree
blooming-blockery block tree duplicate <BLOCK_ID>
blooming-blockery block tree duplicate 1v1

# Delete a subtree (removes block and all descendants)
blooming-blockery block tree delete <BLOCK_ID>
blooming-blockery block tree delete 1v1

# Move block relative to target
blooming-blockery block tree move <SOURCE_ID> <TARGET_ID> --before
blooming-blockery block tree move <SOURCE_ID> <TARGET_ID> --after
blooming-blockery block tree move <SOURCE_ID> <TARGET_ID> --under
```

### Navigation Commands

```bash
# Get next visible block in DFS order
blooming-blockery block nav next <BLOCK_ID>
blooming-blockery block nav next 1v1

# Get previous visible block
blooming-blockery block nav prev <BLOCK_ID>
blooming-blockery block nav prev 2v1

# Get lineage (ancestor chain)
blooming-blockery block nav lineage <BLOCK_ID>
blooming-blockery block nav lineage 1v1

# Search-aware navigation
blooming-blockery block nav find-next <BLOCK_ID> "query"
blooming-blockery block nav find-prev <BLOCK_ID> "query" --no-wrap
```

### Draft Commands (LLM suggestions)

```bash
# Set expansion draft (rewrite + proposed children)
blooming-blockery block draft expand <BLOCK_ID> --rewrite "Refined text" --children "Child 1" "Child 2"
blooming-blockery block draft expand 1v1 --children "Just kids"

# Set reduction draft (condensed version)
blooming-blockery block draft reduce <BLOCK_ID> --reduction "Condensed text" --redundant-children 2v1 3v1

# Set instruction draft (user-authored LLM instructions)
blooming-blockery block draft instruction <BLOCK_ID> --text "Make this more concise"

# Set inquiry draft (LLM response to ask query)
blooming-blockery block draft inquiry <BLOCK_ID> --response "The key insight is..."

# List all drafts for a block
blooming-blockery block draft list <BLOCK_ID>

# Clear drafts (use --all or specific flags)
blooming-blockery block draft clear <BLOCK_ID> --all
blooming-blockery block draft clear <BLOCK_ID> --expand
blooming-blockery block draft clear <BLOCK_ID> --reduce --instruction
```

### Fold (Collapse) Commands

```bash
# Toggle fold state
blooming-blockery block fold toggle <BLOCK_ID>

# Get fold status
blooming-blockery block fold status <BLOCK_ID>
```

### Friend (Cross-reference) Commands

```bash
# Add friend block with optional perspective
blooming-blockery block friend add <TARGET_ID> <FRIEND_ID> --perspective "Related design"
blooming-blockery block friend add <TARGET_ID> <FRIEND_ID> --telescope-lineage --telescope-children

# Remove friend
blooming-blockery block friend remove <TARGET_ID> <FRIEND_ID>

# List friends
blooming-blockery block friend list <TARGET_ID>
```

### Mount (External File) Commands

```bash
# Set mount path (block must be leaf, no children)
blooming-blockery block mount set <BLOCK_ID> <PATH> [--format json|markdown]
blooming-blockery block mount set 1v1 /data/external.json
blooming-blockery block mount set 1v1 /notes/notes.md --format markdown

# Expand mount (load external file)
blooming-blockery block mount expand <BLOCK_ID>

# Collapse mount (remove loaded blocks, restore mount node)
blooming-blockery block mount collapse <BLOCK_ID>

# Move mount backing file and update metadata
blooming-blockery block mount move <BLOCK_ID> <PATH>

# Inline mounted content into current store
blooming-blockery block mount inline <BLOCK_ID>

# Inline all mounts recursively under a subtree
blooming-blockery block mount inline-recursive <BLOCK_ID>

# Extract subtree to external file
blooming-blockery block mount extract <BLOCK_ID> --output <PATH> [--format json|markdown]
blooming-blockery block mount extract 1v1 --output /backup/notes.json

# Persist all expanded mounts to their source files
blooming-blockery block mount save

# Show mount info
blooming-blockery block mount info <BLOCK_ID>
```

### Panel Commands

```bash
# Set block panel state
blooming-blockery block panel set <BLOCK_ID> friends
blooming-blockery block panel set <BLOCK_ID> instruction

# Get block panel state
blooming-blockery block panel get <BLOCK_ID>

# Clear block panel state
blooming-blockery block panel clear <BLOCK_ID>
```

### Context Command

```bash
# Get LLM context for a block (lineage, children, friends)
blooming-blockery block context <BLOCK_ID>
blooming-blockery block context 1v1
```

## Common Error Patterns

- `UnknownBlock`: Block ID not found in store
- `InvalidOperation`:
  - Parent is a mount (cannot add children)
  - Source is ancestor of target (cycle in move)
  - Block has children (cannot set mount)
  - Attempting to add self as friend
- `IoError`: Failed to read/write mount file

## Agent Guidance: Choosing Structure Intentionally

When helping users author specs/docs, prefer these decisions:

- If content answers different design questions, split into sibling branches.
- If content elaborates one parent claim, place it as children.
- Keep the logical chain visible: concept -> spec -> implementation.
- Encourage explicit spec nodes with `requires`, `ensures`, and invariants
  before implementation-heavy branches.
- If two branches should remain separate but mutually relevant, link via friends.
- If friend meaning is unclear, add `--perspective` immediately.
- Before generating drafts, inspect with `block context` and ensure lineage points
  to the intended scope.

## Practical Tips

1. Use `--output json` for scripting and parsing results
2. Block IDs are case-insensitive
3. Mount format defaults to `json` but supports `markdown`
   - `mount extract --format` overrides path-extension inference
4. Batch-capable commands support comma-separated IDs:
   - `show`, `point`
   - `tree add-child|add-sibling|wrap|duplicate|delete|move`
   - `nav next|prev|lineage`
   - `friend add|remove`
   - `fold toggle|status`
   - `draft instruction|inquiry|list|clear`
   - `mount set|expand|collapse|move|extract|inline|inline-recursive|info`
   - `context`
   - For `mount set|move|extract` batch mode, pass a directory-like path
     (`existing/dir` or a path without extension), and each target will use
     `<BLOCK_ID>.<ext>` under that directory.
5. Panel states are `friends` or `instruction`
6. The GUI launches by default if no subcommand is given
7. If users report low-quality LLM responses, first fix tree placement and
   friend/telescope scope before rewriting prompts
