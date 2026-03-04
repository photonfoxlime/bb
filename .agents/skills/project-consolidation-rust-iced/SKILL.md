# SKILL: Codebase Knowledge Consolidation — Rust + iced GUI

## PURPOSE
Extract rewrite-ready knowledge from a Rust/iced GUI codebase across three domains:
1. **Design** — Elm architecture wiring, UI/UX layout, screen inventory, design decisions + rationale
2. **Engineering** — Data structures with ownership semantics, function contracts (requires/ensures/behavior), trait architecture, concurrency design
3. **iced Patterns** — Idiomatic iced usage, Message hierarchy, Commands/Tasks, subscriptions, custom widgets, theming

## TRIGGER
Use when given Rust source files from an iced project and asked to consolidate knowledge for recreation or rewrite.

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
Document all non-trivial concurrency:
- `Command` / `Task` usage — what async work is dispatched and how results feed back as Messages
- `Subscription` sources — what external events are subscribed to (timers, sockets, file watchers)
- Any `Arc<Mutex<>>`, `Arc<RwLock<>>`, channels (`mpsc`, `oneshot`) — document *why* shared state was needed
- `unsafe` blocks — document invariants that justify them
- Threading model: is work offloaded to `tokio`, `rayon`, `std::thread`?

---

## PHASE 3 — ICED PATTERNS & TRICKS

### 3.1 Elm Architecture Wiring
- How `update` is structured (single match, delegating to sub-update functions, etc.)
- How `view` is composed (single function, per-screen view functions, component functions)
- How `Model` is split across screens/modules

### 3.2 Layout Patterns
Document the idioms used:
- `row!` / `column!` / `container` composition style
- Spacing/padding conventions
- Scrollable usage
- Responsive or dynamic layout tricks

### 3.3 Custom Widgets & Canvas
For each custom widget or `Canvas` program:
```
Name:
Purpose:
State: [widget-local state if any]
Draw logic: [what it renders and how]
Events handled: [mouse, keyboard, etc.]
⚠️ Non-obvious: [any tricky geometry, cache invalidation, event propagation]
```

### 3.4 Theming & Styling
- Is a custom `Theme` type used? Document its structure.
- Any custom `StyleSheet` implementations — document what they override and why
- Color palette — extract and name all colors used
- Font loading and usage

### 3.5 iced Version-Specific Notes
- Which version of iced is used (0.12, 0.13, etc.) — note any version-specific APIs
- Any workarounds for known iced limitations (mark ⚠️)
- Feature flags enabled in Cargo.toml and why

---

## OUTPUT

Produce two Markdown documents:

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
### [TypeName]

## Function Contracts
### [fn_name]

## Trait Architecture
### [TraitName]

## Concurrency & Async Design

## iced Patterns
### Layout Conventions
### Custom Widgets
### Theming
### Version Notes
```

---

## NOTES FOR CLAUDE
- The `Message` enum is the single most important artifact — spend extra care on it
- Reconstruct visual layout from `row!`/`column!` nesting into human-readable descriptions
- Infer invariants from `assert!`, `debug_assert!`, `expect()` messages, and type constraints
- Every `Arc<Mutex<>>` and `unsafe` block deserves a documented justification
- Flag anything that fights the borrow checker in a non-obvious way with ⚠️
- Prioritize what a developer needs to *rewrite* this — not just understand it
