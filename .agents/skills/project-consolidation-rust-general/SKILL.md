# SKILL: Codebase Knowledge Consolidation — General Rust Project

## PURPOSE
Extract rewrite-ready knowledge from a Rust codebase across three domains:
1. **Design** — Architecture, module boundaries, data flow, design decisions + rationale
2. **Engineering** — Data structures with ownership semantics, function contracts (requires/ensures/behavior), trait architecture, concurrency design
3. **Rust Patterns** — Idioms, error handling strategy, macro usage, build system, unsafe justifications

## TRIGGER
Use when given Rust source files and asked to consolidate knowledge for recreation, rewrite, or deep understanding.

---

## PHASE 1 — DESIGN KNOWLEDGE

### 1.1 Architecture
- Identify the top-level architectural pattern (pipeline, actor model, layered, plugin-based, CLI, library, etc.)
- Map all crates (if workspace) and modules — their responsibilities and boundaries
- Document how data flows through the system end-to-end
- Note any code generation, build scripts (`build.rs`), or proc macros that affect architecture

### 1.2 Public API Surface (if library)
- Document the intended entry points and their contracts
- Note what is `pub` vs `pub(crate)` vs private and why
- Identify any semver-sensitive design choices

### 1.3 Design Decision Log
```
Decision: [what was chosen]
Alternatives: [if visible in comments, docs, or structure]
Reason: [infer from code, naming, comments, tests]
Impact: [what this enables or prevents]
```
Flag decisions around: crate boundaries, error types, trait vs enum dispatch, sync vs async, serialization format, feature flags.

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

### 2.4 Error Handling Strategy
- What error types are defined? (`thiserror`, `anyhow`, custom enums?)
- How are errors propagated — `?`, `map_err`, `unwrap_or_else`?
- Where are errors handled vs bubbled to the caller?
- Any sentinel values or `Option` used in place of errors — document why

### 2.5 Concurrency & Async Design
Document all non-trivial concurrency:
- Async runtime in use (`tokio`, `async-std`, none) and how it's initialized
- Key `async fn`s and what they await on
- Any `Arc<Mutex<>>`, `Arc<RwLock<>>`, channels (`mpsc`, `oneshot`, `broadcast`) — document *why* shared state was needed
- `unsafe` blocks — document the invariants that justify each one
- Thread pool or `rayon` usage for parallelism

---

## PHASE 3 — RUST PATTERNS & IDIOMS

### 3.1 Ownership & Borrowing Patterns
- Where is cloning used and is it intentional or a workaround?
- Any pervasive use of `Cow<>`, `Box<dyn Trait>`, or `impl Trait` in return position — document why
- Notable lifetime patterns (self-referential structs, `'static` bounds, HRTB)

### 3.2 Error & Option Idioms
- Pervasive combinators used (`map`, `and_then`, `unwrap_or`, `ok_or`)
- Any custom `Result` type alias — document it
- Panic policy: is `unwrap()` banned, allowed in tests only, or used freely?

### 3.3 Macro Usage
For each significant macro (built-in or third-party):
```
Macro: [name]
Source: [std / crate name / custom proc macro]
Purpose: [what it expands to / why it's used]
⚠️ Non-obvious: [any gotchas in its usage here]
```

### 3.4 Build System & Feature Flags
- Cargo features defined — what each enables and any mutual exclusions
- `build.rs` — what it does and why
- Notable dependencies and *why* each was chosen over alternatives
- MSRV (minimum supported Rust version) if noted

### 3.5 Testing Strategy
- Unit vs integration vs doc tests — how they're organized
- Any test helpers, fixtures, or custom assertion macros
- Property-based testing (`proptest`, `quickcheck`) if used
- Mocking approach if any

---

## OUTPUT

Produce two Markdown documents:

**`DESIGN_KNOWLEDGE.md`**
```
# [Project] — Design Knowledge

## Architecture
## Public API Surface
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

## Error Handling Strategy

## Concurrency & Async Design

## Rust Patterns
### Ownership & Borrowing
### Macros
### Build & Features
### Testing
```

---

## NOTES FOR CLAUDE
- Infer invariants from `assert!`, `debug_assert!`, `expect()` messages, and type constraints
- Every `Arc<Mutex<>>` and `unsafe` block deserves a documented justification
- Note clones that look like borrow checker workarounds with ⚠️
- Flag `unwrap()`/`expect()` calls that aren't obviously safe
- Prioritize what a developer needs to *rewrite* this — not just read it
- When a design choice seems surprising, document it — that's the most valuable signal
