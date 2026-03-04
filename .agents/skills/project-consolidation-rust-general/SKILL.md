# SKILL: Codebase Knowledge Consolidation — General Rust Project

## PURPOSE
Extract rewrite-ready knowledge from a Rust codebase across three domains:
1. **Design** — Architecture, module boundaries, data flow, design decisions + rationale
2. **Engineering** — Data structures with ownership semantics, function contracts (requires/ensures/behavior), trait architecture, concurrency design
3. **Rust Patterns** — Idioms, error handling strategy, macro usage, build system, unsafe justifications

## TRIGGER
Use when given Rust source files and asked to consolidate knowledge for recreation, rewrite, or deep understanding.

---

## INVESTIGATIVE PROTOCOL — READ THIS FIRST

Before writing any output, Claude must actively interrogate the codebase by working through every question below. Do not skip questions because the answer seems absent — absence is itself informative and must be noted. For each question, cite the specific file, line, or pattern that provides the answer.

### Architecture & Entry Points
- What is the entry point (`main`, `lib.rs`, workspace root)? What does it do?
- What is the top-level architectural pattern (pipeline, actor, layered, CLI, library, service)?
- If a workspace: what are all crates, what is each responsible for, and how do they depend on each other?
- What is the primary data flow through the system end-to-end? Trace it from input to output.
- Are there any `build.rs` scripts? What do they generate or configure?
- Are there proc macros? What do they expand to?

### Module & Crate Boundaries
- What is the full module tree? List every `mod` declaration.
- What is `pub` vs `pub(crate)` vs private at each boundary, and why?
- Are there any circular dependencies or unusual re-exports?
- Are feature flags used to gate modules or functionality? List all features and what they enable.

### Design Decisions
- Why was this architecture chosen? Look for comments, docs, README, or structural clues.
- Are there any `// TODO`, `// FIXME`, `// HACK`, or `// NOTE` comments? Quote them verbatim.
- Are there commented-out blocks of code? What abandoned approaches do they suggest?
- Are there any surprising type choices or structural decisions that deviate from Rust idioms?
- Are there any places where a simpler approach was clearly available but not taken? Why?

### Engineering — Data Structures
- What are ALL structs and enums defined in the project? For each: purpose, fields/variants, derives, invariants.
- Which types are the most central to the domain model?
- Are there newtype wrappers? What do they enforce?
- Are there any self-referential structs or types with non-trivial lifetimes?
- Are `Arc`, `Rc`, `Cell`, `RefCell`, or `Mutex` used? For each: what does it wrap and why was shared ownership or interior mutability needed?

### Engineering — Functions
- Which functions are the longest or most complex? What do they do?
- What are all public API functions (in `lib.rs` or `pub mod`)? Document each.
- Where is `clone()` called? Is each clone intentional or a borrow checker workaround?
- Where is `unwrap()` or `expect()` called? Is each one justified by an invariant?
- Are there any generic functions? What do the trait bounds express?
- Are there any functions with lifetime parameters? What do the lifetimes represent?

### Trait Architecture
- What custom traits are defined? What abstraction does each provide?
- Which traits have multiple implementors? Why?
- Are `dyn Trait` or `impl Trait` used? Where and why?
- Are any standard traits implemented manually (`Display`, `From`, `Into`, `Iterator`, `Drop`)? What do the impls do?

### Error Handling
- What error types are defined? Are they `thiserror` enums, `anyhow`, or custom?
- Is there a top-level `Result` type alias? What is it?
- How are errors propagated — `?`, `map_err`, `unwrap_or_else`?
- Where are errors handled vs bubbled to the caller?
- Are there any `panic!`, `unwrap()`, or `expect()` calls that encode a policy decision?
- Are `Option` types used where errors might be expected? Why?

### Concurrency & Async
- What async runtime is used (`tokio`, `async-std`, none)? How is it configured?
- What are all `async fn`s? What do they await on?
- Are there `Arc<Mutex<>>` or `Arc<RwLock<>>`? What do they protect and why?
- Are channels used (`mpsc`, `oneshot`, `broadcast`)? What communicates over them?
- Is `rayon` or `std::thread` used for parallelism? For what work?
- Is there any `unsafe`? Quote each block and identify the invariant that justifies it.

### Rust Patterns & Idioms
- Are `Cow<>`, `Box<dyn Trait>`, or `impl Trait` in return position used? Where and why?
- What macros are used (built-in and third-party)? What does each expand to?
- Are there any notable iterator chains? What do they compute?
- What is the `unwrap()` policy — banned, test-only, or freely used?
- Are there any HRTB (`for<'a>`) or other advanced lifetime patterns?

### Testing
- What is the testing strategy — unit, integration, doc tests?
- Are there test helpers, fixtures, or custom assertion macros?
- Is property-based testing used (`proptest`, `quickcheck`)?
- What is the coverage of the most critical paths?
- Are there any tests that document known edge cases or bugs?

### Build & Dependencies
- What are all dependencies in `Cargo.toml`? For each non-obvious one: why was it chosen?
- What is the MSRV (minimum supported Rust version)?
- Are there any patched or git dependencies? Why?
- What Cargo features are defined and what do they gate?

---

## PHASE 1 — DESIGN KNOWLEDGE

### 1.1 Architecture
- Top-level pattern and crate/module map
- End-to-end data flow
- Public API surface (if library)
- `build.rs` and proc macro effects

### 1.2 Design Decision Log
```
Decision: [what was chosen]
Alternatives: [if visible]
Reason: [infer from code, naming, comments]
Impact: [what this enables or prevents]
```

---

## PHASE 2 — ENGINEERING KNOWLEDGE

### 2.1 Data Structure Contracts
```
Name:
Kind: [struct / enum / newtype / type alias]
Purpose:
Fields/Variants: [(name, type, meaning)]
Derives: [note *why* each matters]
Ownership notes: [Arc/Rc, interior mutability]
Invariants:
Lifetime params:
Lifecycle:
```

### 2.2 Function & Method Contracts
```
Signature: fn name<'a, T: Bound>(param: &'a Type) -> ReturnType
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
Design intent: [trait vs enum dispatch vs generics]
```

### 2.4 Error Handling Strategy
- Error types and their hierarchy
- Propagation patterns
- Panic policy

### 2.5 Concurrency & Async Design
- Async runtime and configuration
- All async operations and what they await
- Shared state and why it's shared
- `unsafe` blocks with invariant justifications

---

## PHASE 3 — RUST PATTERNS & IDIOMS

### 3.1 Ownership & Borrowing Patterns
- Clones: intentional vs workaround
- `Cow`, `Box<dyn>`, `impl Trait` usage
- Notable lifetime patterns

### 3.2 Error & Option Idioms
- Combinator patterns used
- Custom `Result` aliases
- Panic policy

### 3.3 Macro Usage
```
Macro: [name]
Source: [std / crate / custom]
Purpose:
⚠️ Non-obvious:
```

### 3.4 Build System & Feature Flags
- All Cargo features and what they gate
- `build.rs` behavior
- Notable dependency choices

### 3.5 Testing Strategy
- Test organization and coverage
- Fixtures and helpers
- Property-based testing

---

## OUTPUT

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
## Function Contracts
## Trait Architecture
## Error Handling Strategy
## Concurrency & Async Design
## Rust Patterns
```

---

## FINAL NOTES FOR CLAUDE
- Work through EVERY question in the Investigative Protocol before writing output
- Cite file and line references for non-obvious findings
- Quote TODO/FIXME/HACK/NOTE comments verbatim — they are design documentation
- Flag every `unwrap()`, `unsafe`, and `Arc<Mutex<>>` — each one needs a documented justification
- If a question has no answer in the codebase, write "not found" — do not skip it
- Flag anything surprising or non-idiomatic with ⚠️
- The goal is: a developer with zero prior context can recreate this project from these docs alone
