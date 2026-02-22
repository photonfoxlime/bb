# Expansion Draft Diff Rendering

For overall architecture and document index, see [architecture.md](architecture.md).

## Overview

Expansion drafts use a git-diff-style format for the rewrite section only, showing word-wise changes between old and new text. Child suggestions are displayed as a simple list without diff highlighting.

## Visual Design

### Rewrite Section

When `draft.rewrite` is present, show a unified diff view with word-wise highlighting:

```
┌─────────────────────────────────────────────────────────────┐
│ Rewrite                                                     │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ Original text continues with some changes               │ │
│ │   [red highlight on "some"]                             │ │
│ │                                                         │ │
│ │ New rewritten text continues differently with changes   │ │
│ │   [green highlight on "differently"]                    │ │
│ └─────────────────────────────────────────────────────────┘ │
│ [Apply rewrite] [Dismiss rewrite]                           │
└─────────────────────────────────────────────────────────────┘
```

Where words are highlighted inline:
- Old: "Original text continues with [some] changes" (red highlight on deleted words)
- New: "New rewritten text continues [differently] with changes" (green highlight on added words)

**Word-wise diff:**
- **Deletions**: Red background (`Color { a: 0.08, ..DANGER }`) on removed words
- **Additions**: Green background (`Color { a: 0.08, ..SUCCESS }`) on added words
- **Context** (unchanged): Neutral background, normal text
- **Layout**: Old text shown first (with deletions highlighted), then new text (with additions highlighted)
- **Inline highlighting**: Words are highlighted within their lines, preserving natural text flow

### Children Section

Child suggestions are displayed as a simple list (no diff view):

```
┌─────────────────────────────────────────────────────────────┐
│ Child suggestions                                           │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ First suggested child block text                        │ │
│ │ [Keep] [Drop]                                           │ │
│ ├─────────────────────────────────────────────────────────┤ │
│ │ Second suggested child block text                       │ │
│ │ [Keep] [Drop]                                           │ │
│ └─────────────────────────────────────────────────────────┘ │
│ [Accept all] [Discard all]                                  │
└─────────────────────────────────────────────────────────────┘
```

Each child:
- Simple text display (no diff highlighting)
- Individual "Keep" / "Drop" buttons per child
- "Accept all" / "Discard all" buttons at section header
- Note: Children are new additions only, so no diff comparison needed

## Implementation

### Diff Algorithm

**Rewrite section only:** Use word-wise diffing to highlight changes at the word level:
1. Tokenize old/new text into words (split on whitespace, preserve punctuation)
2. Compute word-by-word differences (insertions, deletions, unchanged)
3. Render with inline word-level highlighting

**Dependencies:**
- Use `similar` crate for word-level diff (Myers algorithm on word sequences)
- Or implement simple word-by-word comparison for MVP
- Consider preserving whitespace/punctuation as separate tokens for accurate alignment

**Children section:** No diff algorithm needed; display as simple text list.

### Styling

**New theme functions:**
- `diff_deletion`: Red-tinted background for removed words (inline spans)
- `diff_addition`: Green-tinted background for added words (inline spans)
- `diff_context`: Neutral background for unchanged words

**Color scheme:**
- Deletions: `Color { a: 0.08, ..DANGER }` background on word spans
- Additions: `Color { a: 0.08, ..SUCCESS }` background on word spans
- Text: `INK` color
- Use inline text spans with background styling for word-level highlighting

### Layout Structure

```rust
fn render_expansion_panel(...) -> Element {
    column![]
        .spacing(6)
        .push(render_rewrite_diff(...))  // if rewrite present
        .push(render_children_diff(...))  // if children present
        .push(render_action_buttons(...))
}
```

**Rewrite diff:**
- Header: "Rewrite" label
- Diff view: Old text (with deletions highlighted) followed by new text (with additions highlighted)
- Word-level highlighting: Inline spans with colored backgrounds
- Action buttons: Apply/Dismiss

**Children section:**
- Header: "Child suggestions" + Accept all/Discard all buttons
- Simple list display: Each child shown as plain text (no diff view)
- Per-child Keep/Drop buttons

## User Experience

**Benefits:**
- Clear visual distinction between old and new rewrite content at word level
- Easy to see exactly which words changed in the rewrite
- Preserves natural text flow (no line breaks for diff markers)
- More granular than line-wise diff for understanding precise changes
- Children remain simple and easy to scan without diff complexity

**Considerations:**
- Word tokenization must handle punctuation and whitespace correctly
- For very long texts, consider truncating or scrolling
- Ensure diff algorithm handles edge cases (empty text, single word, punctuation-only changes)
- May need to handle multi-word phrases that changed together as units
- Diff view applies only to rewrite section; children use simple list display
