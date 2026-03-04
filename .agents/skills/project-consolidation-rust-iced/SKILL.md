# SKILL: Codebase Knowledge Consolidation — Rust + iced GUI

## PURPOSE
Extract rewrite-ready knowledge from a Rust/iced GUI codebase across three domains:
1. **Design** — Elm architecture wiring, UI/UX layout, screen inventory, design decisions + rationale
2. **Engineering** — Data structures with ownership semantics, function contracts (requires/ensures/behavior), trait architecture, concurrency design
3. **iced Patterns** — Idiomatic iced usage, Message hierarchy, Commands/Tasks, subscriptions, custom widgets, theming

## TRIGGER
Use when given Rust source files from an iced project and asked to consolidate knowledge for recreation or rewrite.

---

## INVESTIGATIVE PROTOCOL — READ THIS FIRST

Before writing any output, Claude must actively interrogate the codebase by working through every question below. Do not skip questions because the answer seems absent — absence is itself informative and must be noted. For each question, cite the specific file, line, or pattern that provides the answer.

### Architecture & State
- What is the top-level type that implements `Application` or `Sandbox`? What fields does it have and what does each represent?
- How is the application state split across the model? Are there nested sub-states? What owns what?
- Are there multiple "pages" or "screens" modeled? How is the active screen tracked (enum variant, bool flags, index)?
- Is there any state that lives outside the main model (globals, `lazy_static`, `once_cell`, thread-locals)? Why?
- What is initialized at startup vs lazily? What can fail at startup?

### Message Hierarchy
- What are ALL variants of the top-level `Message` enum? List every one.
- Which variants carry payloads? What are those types?
- Are there nested message enums (e.g. `Message::Screen(ScreenMsg)`)? Enumerate the full tree.
- Which messages are produced by user interaction vs async results vs subscriptions vs timers?
- Are there any messages that feel redundant, overly coarse, or overly fine-grained? Note these — they reveal design tension.
- Which `update` arms have the most complex logic? What do they do?

### UI/UX — Exhaustive Screen Analysis
For EVERY screen, panel, modal, overlay, and popover — no matter how minor:
- What is the exact widget hierarchy? Trace every `row!`, `column!`, `container`, `scrollable` from outermost to innermost.
- What are all interactive elements (buttons, text inputs, sliders, checkboxes, pick lists, etc.)? What message does each produce?
- What state is reflected visually (enabled/disabled, selected, loading, error)? How is that state stored?
- Are any widgets conditionally shown or hidden? Under what conditions?
- What text is displayed? Is it static, dynamic, or formatted? Where does it come from?
- Are there any custom-drawn elements (Canvas, custom widget)? What do they render?
- What are the exact spacing, padding, and sizing values used? Are they constants or hardcoded?
- What fonts are used? Are any loaded from files?
- What colors appear? Map every color to its hex/RGB value and where it's used.
- Are there icons or images? How are they loaded and rendered?
- What happens on window resize? Is layout responsive or fixed?
- Are there any animations or transitions?
- What keyboard shortcuts or focus behaviors exist?

### Design Decisions
- Why was this screen/component structure chosen over alternatives? Look for comments, TODOs, or structural clues.
- Are there any `// TODO`, `// FIXME`, `// HACK`, or `// NOTE` comments? Quote them — they are explicit design signals.
- Are there any commented-out blocks of code? What do they suggest about abandoned approaches?
- Are there any unusually complex `update` arms that suggest a difficult design problem was solved here?
- Are there any surprising type choices (e.g. `String` where `&str` might work, `Vec` where a `HashMap` might be expected)?

### Engineering
- What are ALL structs and enums defined in the project? For each: purpose, fields/variants, derives, invariants.
- Which functions are longest or most complex? What do they do?
- Where is `clone()` called? Is each clone intentional (shared ownership) or a borrow checker workaround?
- Where is `unwrap()` or `expect()` called? Is each one justified?
- Are there lifetime parameters? What do they represent?
- Are there any custom traits? What do they abstract?
- Are there generic functions? What do the bounds express?

### Concurrency & Async
- What `Command`s or `Task`s are constructed? What async work do they perform?
- What `Subscription`s are active? What events do they listen for?
- Is `Arc<Mutex<>>` or `Arc<RwLock<>>` used anywhere? What shared state does it protect and why?
- Are there any channels (`mpsc`, `oneshot`)? What communicates over them?
- Is there any `unsafe`? What invariant justifies it?
- What async runtime is used? How is it configured?

### iced-Specific
- Which version of iced is used? Which feature flags are enabled in `Cargo.toml`?
- Are there any custom widgets (types implementing `Widget`)? Describe each fully.
- Are there any `Canvas` programs (types implementing `Program`)? What do they draw?
- Is a custom `Theme` type defined? What does it contain?
- Are there custom `StyleSheet` implementations? What do they override?
- Are there any workarounds for iced limitations? (Look for unusual patterns, wrapper types, or comments.)
- Are `view` functions split into helpers? How is the view layer organized?

---

## PHASE 1 — DESIGN KNOWLEDGE

### 1.1 Application Architecture
- Identify the top-level `Application` or `Sandbox` impl and its associated `Model`, `Message`, `update`, `view`
- Map all sub-screens, pages, or modes (often modeled as an enum in the state)
- Document state ownership: what owns what, and how sub-state is accessed
- Note any multi-window setup, overlay layers, or modal patterns

### 1.2 Message Enum Hierarchy
The `Message` enum tree *is* the interaction design. Document it fully:
```
Message
├── Variant(payload) — [what triggers this] → [what update does]
├── SubModule(SubMessage)
│   ├── ...
```
For each leaf variant: what user action or async event produces it, and what state change it causes.

### 1.3 UI/UX Screen Inventory
For every major screen/view/page:
```
Screen: [name]
Purpose: [what the user accomplishes here]
Layout: [reconstruct from row!/column!/container! nesting — describe visually]
Key widgets: [buttons, inputs, lists, canvases — and their roles]
Navigation: [what Message transitions to/from this screen]
Visual conventions: [colors, spacing, padding, fonts]
Shortcuts / interactions: [keyboard events, scroll, drag]
```

### 1.4 Design Decision Log
```
Decision: [what was chosen]
Alternatives: [if visible in comments or structure]
Reason: [infer from code, naming, comments]
Impact: [what this enables or prevents]
```
Flag decisions around: state shape, message granularity, sync vs async, widget reuse, theme approach.

---

## PHASE 2 — ENGINEERING KNOWLEDGE

### 2.1 Data Structure Contracts
For each important `struct`, `enum`, or `type` alias:
```
Name:
Kind: [struct / enum / newtype / type alias]
Purpose:
Fields/Variants: [(name, type, meaning)]
Derives: [Copy, Clone, Debug, PartialEq, serde, etc. — note *why* each matters]
Ownership notes: [owned vs borrowed fields, Arc/Rc usage, interior mutability]
Invariants: [conditions always true — e.g. "vec is never empty", "index < items.len()"]
Lifetime params: [if any — what they represent]
Lifecycle: [how created, mutated, dropped]
```

### 2.2 Function & Method Contracts
For each public or architecturally important function:
```
Signature: fn name<'a, T: Bound>(param: &'a Type) -> ReturnType
Purpose: [one line]
Trait bounds: [what each bound requires and why it was chosen]
Requires: [preconditions on inputs and program state]
Ensures: [postconditions — what is guaranteed on return]
Behavior: [logic summary, side effects, error paths]
Panics: [any unwrap/expect/index — conditions under which they're safe]
⚠️ Gotchas: [lifetime pitfalls, borrow checker traps, perf notes]
```

### 2.3 Trait Architecture
For each custom trait or significant trait impl:
```
Trait: [name]
Purpose: [what abstraction it provides]
Implementors: [which types implement it and why]
Key methods: [signatures + behavioral contracts]
Design intent: [why a trait vs enum dispatch vs generics]
```

### 2.4 Concurrency & Async Design
- `Command` / `Task` usage — what async work is dispatched and how results feed back as Messages
- `Subscription` sources — what external events are subscribed to
- Any `Arc<Mutex<>>`, `Arc<RwLock<>>`, channels — document *why* shared state was needed
- `unsafe` blocks — document invariants that justify them
- Threading model: `tokio`, `rayon`, `std::thread`?

---

## PHASE 3 — ICED PATTERNS & TRICKS

### 3.1 Elm Architecture Wiring
- How `update` is structured and composed
- How `view` is split into helper functions
- How `Model` is divided across modules

### 3.2 Layout Patterns
- `row!` / `column!` / `container` composition style
- Spacing/padding conventions (named constants vs magic numbers)
- Scrollable usage
- Responsive or conditional layout tricks

### 3.3 Custom Widgets & Canvas
```
Name:
Purpose:
State: [widget-local state if any]
Draw logic: [what it renders and how]
Events handled: [mouse, keyboard, etc.]
⚠️ Non-obvious: [tricky geometry, cache invalidation, event propagation]
```

### 3.4 Theming & Styling
- Custom `Theme` type structure
- Custom `StyleSheet` implementations — what they override and why
- Full color palette with names and hex values
- Font loading and usage

### 3.5 iced Version-Specific Notes
- iced version and feature flags
- Workarounds for iced limitations (⚠️)

---

## OUTPUT

**`DESIGN_KNOWLEDGE.md`**
```
# [Project] — Design Knowledge
## Application Architecture
## Message Hierarchy
## UI/UX Screen Inventory
### [Screen]
## Design Decision Log
| Decision | Reason | Impact |
```

**`ENGINEERING_KNOWLEDGE.md`**
```
# [Project] — Engineering Knowledge
## Data Structures
## Function Contracts
## Trait Architecture
## Concurrency & Async Design
## iced Patterns
```

---

## FINAL NOTES FOR CLAUDE
- Work through EVERY question in the Investigative Protocol before writing output
- Cite file and line references for non-obvious findings
- Quote TODO/FIXME/HACK comments verbatim — they are design documentation
- Reconstruct visual layout from widget hierarchy into human-readable prose
- Flag anything surprising, non-idiomatic, or that fights the borrow checker with ⚠️
- If a question has no answer in the codebase, write "not found" — do not skip it
- The goal is: a developer with zero prior context can recreate this project from these docs alone
