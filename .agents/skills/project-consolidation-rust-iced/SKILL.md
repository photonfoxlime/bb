# SKILL: Codebase Knowledge Consolidation — Rust + iced GUI

## PURPOSE
Extract rewrite-ready knowledge from a Rust/iced GUI codebase across four domains:
1. **Functionality** — What the software does: every user-facing feature and implemented behavior
2. **Design** — Elm architecture wiring, UI/UX layout, screen inventory, design decisions + rationale
3. **Engineering** — Data structures with ownership semantics, function contracts, trait architecture, concurrency
4. **iced Patterns** — Idiomatic iced usage, Message hierarchy, Commands/Tasks, subscriptions, custom widgets, theming

## TRIGGER
Use when given Rust source files from an iced project and asked to consolidate knowledge for recreation or rewrite.

---

## INVESTIGATIVE PROTOCOL — READ THIS FIRST

Before writing any output, work through every question below. Do not skip questions because the answer seems absent — absence is itself informative and must be noted. Cite the specific file, line, or pattern that provides each answer.

### Functionality & Features
- What is the stated purpose of this application? (README, top-level docs, or inferred from code)
- What can a user *do* with this application? List every distinct action or workflow end-to-end.
- What data does the application operate on? Where does it come from (files, network, user input, system)?
- What does the application produce or output (files, UI state, network requests, system effects)?
- Are there distinct modes of operation (e.g. view vs edit, online vs offline, debug vs release behavior)?
- What features are fully implemented vs partially implemented vs stubbed? (Look for TODOs, unimplemented!(), todo!())
- Are there any hidden, debug-only, or experimental features? (Look for cfg(debug_assertions), feature flags, commented-out UI)
- What are the most important or complex features from an implementation standpoint?
- What edge cases or error conditions are explicitly handled in the logic?
- Are there any features that appear to have been removed or abandoned? (Commented-out code, dead functions, orphaned messages)

### Architecture & State
- What is the top-level type that implements `Application` or `Sandbox`? What fields does it have and what does each represent?
- How is the application state split across the model? Are there nested sub-states? What owns what?
- Are there multiple "pages" or "screens" modeled? How is the active screen tracked?
- Is there any state that lives outside the main model (globals, `lazy_static`, `once_cell`, thread-locals)? Why?
- What is initialized at startup vs lazily? What can fail at startup?

### Message Hierarchy
- What are ALL variants of the top-level `Message` enum? List every one.
- Which variants carry payloads? What are those types?
- Are there nested message enums? Enumerate the full tree.
- Which messages are produced by user interaction vs async results vs subscriptions vs timers?
- Are there any messages that feel redundant, overly coarse, or overly fine-grained? Note these.
- Which `update` arms have the most complex logic? What do they do?

### UI/UX — Exhaustive Screen Analysis
For EVERY screen, panel, modal, overlay, and popover — no matter how minor:
- What is the exact widget hierarchy? Trace every `row!`, `column!`, `container`, `scrollable` from outermost to innermost.
- What are all interactive elements (buttons, text inputs, sliders, checkboxes, pick lists)? What message does each produce?
- What state is reflected visually (enabled/disabled, selected, loading, error)? How is that state stored?
- Are any widgets conditionally shown or hidden? Under what conditions?
- What text is displayed? Is it static, dynamic, or formatted? Where does it come from?
- Are there any custom-drawn elements (Canvas, custom widget)? What do they render?
- What are the exact spacing, padding, and sizing values? Are they constants or hardcoded?
- What fonts are used? Are any loaded from files?
- What colors appear? Map every color to its hex/RGB value and where it's used.
- Are there icons or images? How are they loaded and rendered?
- What happens on window resize? Is layout responsive or fixed?
- Are there any animations or transitions?
- What keyboard shortcuts or focus behaviors exist?

### Design Decisions
- Why was this screen/component structure chosen? Look for comments, TODOs, or structural clues.
- Are there any `// TODO`, `// FIXME`, `// HACK`, or `// NOTE` comments? Quote them verbatim.
- Are there any commented-out blocks of code? What do they suggest about abandoned approaches?
- Are there any unusually complex `update` arms that suggest a hard design problem was solved here?
- Are there any surprising type choices?

### Engineering
- What are ALL structs and enums defined in the project? For each: purpose, fields/variants, derives, invariants.
- Which functions are longest or most complex? What do they do?
- Where is `clone()` called? Is each clone intentional or a borrow checker workaround?
- Where is `unwrap()` or `expect()` called? Is each one justified?
- Are there lifetime parameters? What do they represent?
- Are there any custom traits? What do they abstract?

### Concurrency & Async
- What `Command`s or `Task`s are constructed? What async work do they perform?
- What `Subscription`s are active? What events do they listen for?
- Is `Arc<Mutex<>>` or `Arc<RwLock<>>` used? What does it protect and why?
- Are there any channels? What communicates over them?
- Is there any `unsafe`? What invariant justifies it?

### iced-Specific
- Which version of iced is used? Which feature flags are enabled?
- Are there custom widgets? Describe each fully.
- Are there `Canvas` programs? What do they draw?
- Is a custom `Theme` type defined? What does it contain?
- Are there custom `StyleSheet` implementations? What do they override?
- Are there any workarounds for iced limitations?
- How are `view` functions organized and split?

---

## PHASE 0 — FUNCTIONALITY INVENTORY

This phase must be completed first. It answers: **what does this software actually do?**

### 0.1 Application Purpose
One-paragraph summary of what this application is and what problem it solves.

### 0.2 User-Facing Feature List
An exhaustive list of every feature a user can access or experience. For each:
```
Feature: [name]
Description: [what it does from the user's perspective]
Entry point: [how the user accesses it — menu, button, shortcut, etc.]
Status: [fully implemented / partial / stubbed / appears abandoned]
Notes: [any limitations, known issues, or TODOs attached to it]
```

### 0.3 Operational Modes
Document any distinct modes of operation:
```
Mode: [name]
Trigger: [how it's entered]
Behavior differences: [what changes in this mode]
```

### 0.4 Data & I/O
- What data formats does the app read? (files, network, stdin, clipboard, etc.)
- What does it write or produce?
- What external systems does it talk to?
- What persists between sessions?

### 0.5 Implemented Algorithms & Logic
List every non-trivial algorithm, computation, or business logic rule implemented:
```
Algorithm/Logic: [name or description]
Location: [file/function]
Purpose: [what it computes or decides]
Complexity/notes: [anything non-obvious]
```

### 0.6 Unimplemented / Abandoned Features
- Quote all `todo!()`, `unimplemented!()`, and `// TODO` markers
- List any dead code paths or orphaned `Message` variants that go nowhere
- Note any features that appear started but incomplete

---

## PHASE 1 — DESIGN KNOWLEDGE

### 1.1 Application Architecture
- Top-level `Application`/`Sandbox` impl, `Model`, `Message`, `update`, `view`
- Sub-screens, pages, or modes and how they're tracked
- State ownership map
- Multi-window, overlay, or modal patterns

### 1.2 Message Enum Hierarchy
```
Message
├── Variant(payload) — [trigger] → [update effect]
├── SubModule(SubMessage)
│   ├── ...
```

### 1.3 UI/UX Screen Inventory
```
Screen: [name]
Purpose:
Layout: [visual description of widget nesting]
Key widgets: [with roles and messages produced]
Navigation: [Message transitions in/out]
Visual conventions: [colors, spacing, fonts]
Shortcuts:
```

### 1.4 Design Decision Log
```
Decision:
Alternatives:
Reason:
Impact:
```

---

## PHASE 2 — ENGINEERING KNOWLEDGE

### 2.1 Data Structure Contracts
```
Name:
Kind:
Purpose:
Fields/Variants:
Derives: [with reasons]
Ownership notes:
Invariants:
Lifetime params:
Lifecycle:
```

### 2.2 Function & Method Contracts
```
Signature:
Purpose:
Trait bounds:
Requires:
Ensures:
Behavior:
Panics:
⚠️ Gotchas:
```

### 2.3 Trait Architecture
```
Trait:
Purpose:
Implementors:
Key methods:
Design intent:
```

### 2.4 Concurrency & Async Design
| Mechanism | Where used | Why |
|-----------|-----------|-----|
| `Command`/`Task` | | |
| `Subscription` | | |
| `Arc<Mutex<>>` | | |
| `unsafe` | | |

---

## PHASE 3 — ICED PATTERNS

### 3.1 Elm Architecture Wiring
### 3.2 Layout Patterns
### 3.3 Custom Widgets & Canvas
```
Name:
Purpose:
State:
Draw logic:
Events:
⚠️ Non-obvious:
```
### 3.4 Theming & Styling
- Custom `Theme` structure
- Full color palette (name → hex)
- Font usage
- `StyleSheet` overrides

### 3.5 iced Version Notes & Workarounds

---

## OUTPUT DOCUMENTS

**`FUNCTIONALITY.md`** ← new, produced first
```
# [Project] — Functionality Inventory
## Purpose
## Feature List
## Operational Modes
## Data & I/O
## Implemented Algorithms
## Unimplemented / Abandoned
```

**`DESIGN_KNOWLEDGE.md`**
```
# [Project] — Design Knowledge
## Architecture
## Message Hierarchy
## UI/UX Screen Inventory
## Design Decision Log
```

**`ENGINEERING_KNOWLEDGE.md`**
```
# [Project] — Engineering Knowledge
## Data Structures
## Function Contracts
## Trait Architecture
## Concurrency & Async
## iced Patterns
```

---

## FINAL NOTES FOR CLAUDE
- Produce `FUNCTIONALITY.md` first — it frames everything else
- Work through EVERY question in the Investigative Protocol before writing
- Cite file and line references for non-obvious findings
- Quote TODO/FIXME/HACK/NOTE/unimplemented!()/todo!() verbatim — they are documentation
- Reconstruct visual layout from widget hierarchy into human-readable prose
- Flag anything surprising or non-idiomatic with ⚠️
- If a question has no answer, write "not found" — never skip
- The goal is: a developer with zero prior context can recreate this project from these docs alone
