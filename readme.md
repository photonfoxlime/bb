# bb

A document editor that catalyzes text-based design and implementation.

## Core Concepts

The document is tree-structured where each node (which we call *block*) contains
- An optional terminal element (which we call *point*, typically a line of text), and
- A list of trees (which we call *forest*), plus
- Some other attributes, like whether this block has named reference(s),
  or whether this block should be opened as a document itself.

The root is just a block. Higher-order blocks are naturally implemented as sub-blocks.

## Typical Workflow

1. The user starts by writing the first "line" in the document as a small prompt,
   which is also the *point* of the first *block*. 
2. The user clicks *expand* button that's attached to each block.
   The line is sent to LLM to get a structured and elaborated response:
   - (Potentially) a suggestion to rewrite the current *point*.
   - A few sub-blocks that is logically direct elaborations of the idea described.
     - All sub-blocks must only contain one concise and readable *point*,
       and no subsequent forests.
     - Longer responses can be cached in the sub-blocks' attributes,
       but NOT immediately visible to the user.
3. The user keeps ideal sub-blocks (that are actually sub-points) and
   discards undesirable ones.
4. The user can then choose to develop on one of the sub-blocks by editing,
   effectively back to step 1.
5. Alternatively, the user can further *expand* on the initial *point* to
   retrieve more inspirations from the LLM, and then back to step 3.
6. Alternatively, the user can summarize a verbose point to a concise one,
   by clicking the *reduce* button that's attached to each block.

## UI Draft

The UI renders the document as a calm, handwritten-feeling tree. Each level is a
vertical line (a structural spine), and every block is marked by the same simple
dot placed on that line. The text for each block sits to the right of its dot,
with small inline actions that feel like annotations rather than heavy controls.

Multiple spines are expected: nested blocks appear as a second (or third) column
to the right, showing the tree structure through alignment and spacing instead
of changing dot styles. The lines are not timelines; they are visual hints for
parent/child structure only.

The overall aesthetic is light and airy: soft blue ink, a paper-like background,
and generous whitespace. The interface prioritizes legibility and flow so the
structure of ideas is more prominent than chrome.
