---
name: blooming-blockery-cli
description: Documents the blooming-blockery (block) CLI contract and provides correct command patterns for block store operations.
---

# blooming-blockery-cli

Use this skill to run `block` sub-commands of `blooming-blockery` correctly,
and understand the `blooming-blockery` CLI structure.

## When to use

- The user asks how to use `blooming-blockery` or `block` from terminal scripts.
- The user hits CLI parsing errors with block IDs, mount formats, or panel states.
- The user needs ready-to-copy command examples for block store workflows.
- The user is scripting interactions with the block document store.

## Global Flags

All commands support these global flags:

- `--store <PATH>`: Path to the block store file (defaults to app data file)
- `--verbose`: Enable verbose output (currently reserved)
- `--output <FORMAT>`: Output format - `table` (default) or `json`

## Block ID Format

Block IDs use a clean format like `1v1`, `2v3` where:
- First number = slot index in the store
- `v` = separator
- Second number = generation counter (increments on reuse)

## Command Reference

All the following commands should be prepended by `blooming-blockery`.

### Query Commands

```bash
# List all root block IDs
block roots
block roots --output json

# Show block details
block show <BLOCK_ID>
block show 1v1 --output json

# Search blocks by text (case-insensitive substring)
block find "search query"
block find "TODO" --limit 10

# Edit the text content of a block
block point <BLOCK_ID> "New text content"
block point 1v1 "Updated text"
```

### Tree Structure Commands

```bash
# Add child block under parent (parent must not be a mount)
block tree add-child <PARENT_ID> "Text content"
block tree add-child 1v1 "My new idea"

# Add sibling after a block
block tree add-sibling <BLOCK_ID> "Text content"
block tree add-sibling 1v1 "Next sibling"

# Wrap a block with a new parent
block tree wrap <BLOCK_ID> "Parent text"
block tree wrap 1v1 "New parent section"

# Duplicate a subtree
block tree duplicate <BLOCK_ID>
block tree duplicate 1v1

# Delete a subtree (removes block and all descendants)
block tree delete <BLOCK_ID>
block tree delete 1v1

# Move block relative to target
block tree move <SOURCE_ID> <TARGET_ID> --before
block tree move <SOURCE_ID> <TARGET_ID> --after
block tree move <SOURCE_ID> <TARGET_ID> --under
```

### Navigation Commands

```bash
# Get next visible block in DFS order
block nav next <BLOCK_ID>
block nav next 1v1

# Get previous visible block
block nav prev <BLOCK_ID>
block nav prev 2v1

# Get lineage (ancestor chain)
block nav lineage <BLOCK_ID>
block nav lineage 1v1
```

### Draft Commands (LLM suggestions)

```bash
# Set expansion draft (rewrite + proposed children)
block draft expand <BLOCK_ID> --rewrite "Refined text" --children "Child 1" "Child 2"
block draft expand 1v1 --children "Just kids"

# Set reduction draft (condensed version)
block draft reduce <BLOCK_ID> --reduction "Condensed text" --redundant-children 2v1 3v1

# Set instruction draft (user-authored LLM instructions)
block draft instruction <BLOCK_ID> --text "Make this more concise"

# Set inquiry draft (LLM response to ask query)
block draft inquiry <BLOCK_ID> --response "The key insight is..."

# List all drafts for a block
block draft list <BLOCK_ID>

# Clear drafts (use --all or specific flags)
block draft clear <BLOCK_ID> --all
block draft clear <BLOCK_ID> --expand
block draft clear <BLOCK_ID> --reduce --instruction
```

### Fold (Collapse) Commands

```bash
# Toggle fold state
block fold toggle <BLOCK_ID>

# Get fold status
block fold status <BLOCK_ID>
```

### Friend (Cross-reference) Commands

```bash
# Add friend block with optional perspective
block friend add <TARGET_ID> <FRIEND_ID> --perspective "Related design"
block friend add <TARGET_ID> <FRIEND_ID> --telescope-lineage --telescope-children

# Remove friend
block friend remove <TARGET_ID> <FRIEND_ID>

# List friends
block friend list <TARGET_ID>
```

### Mount (External File) Commands

```bash
# Set mount path (block must be leaf, no children)
block mount set <BLOCK_ID> <PATH> [--format json|markdown]
block mount set 1v1 /data/external.json
block mount set 1v1 /notes/notes.md --format markdown

# Expand mount (load external file)
block mount expand <BLOCK_ID>

# Collapse mount (remove loaded blocks, restore mount node)
block mount collapse <BLOCK_ID>

# Move mount backing file and update metadata
block mount move <BLOCK_ID> <PATH>

# Inline mounted content into current store
block mount inline <BLOCK_ID>

# Inline all mounts recursively under a subtree
block mount inline-recursive <BLOCK_ID>

# Extract subtree to external file
block mount extract <BLOCK_ID> --output <PATH> [--format json|markdown]
block mount extract 1v1 --output /backup/notes.json

# Persist all expanded mounts to their source files
block mount save

# Show mount info
block mount info <BLOCK_ID>
```

### Panel Commands

```bash
# Set panel state
block panel set <BLOCK_ID> friends
block panel set <BLOCK_ID> instruction

# Get panel state
block panel get <BLOCK_ID>

# Clear panel state
block panel clear <BLOCK_ID>
```

### Context Command

```bash
# Get LLM context for a block (lineage, children, friends)
block context <BLOCK_ID>
block context 1v1
```

## Common Error Patterns

- `UnknownBlock`: Block ID not found in store
- `InvalidOperation`: 
  - Parent is a mount (cannot add children)
  - Source is ancestor of target (cycle in move)
  - Block has children (cannot set mount)
  - Attempting to add self as friend
- `IoError`: Failed to read/write mount file

## Tips

1. Use `--output json` for scripting and parsing results
2. Block IDs are case-insensitive
3. Mount format defaults to `json` but supports `markdown`
   - `mount extract --format` overrides path-extension inference
4. Panel states are `friends` or `instruction`
5. The GUI launches by default if no subcommand is given
