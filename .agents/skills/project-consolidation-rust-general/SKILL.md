---
name: project-consolidation-rust-general
description: Extract rewrite-ready knowledge from general Rust codebases — functionality, design, engineering, Rust patterns. Use when given Rust source and asked to consolidate for recreation or rewrite.
---

# SKILL: Codebase Knowledge Consolidation — General Rust Project

## PURPOSE
Extract rewrite-ready knowledge from a Rust codebase across four domains:
1. **Functionality** — What the software does: every feature and implemented behavior
2. **Design** — Architecture, module boundaries, data flow, design decisions + rationale
3. **Engineering** — Data structures, function contracts, trait architecture, concurrency
4. **Rust Patterns** — Idioms, error handling, macro usage, build system, unsafe justifications

## TRIGGER
Use when given Rust source files and asked to consolidate knowledge for recreation, rewrite, or deep understanding.

---

## INVESTIGATIVE PROTOCOL — READ THIS FIRST

Before writing any output, work through every question below. Do not skip questions because the answer seems absent — absence is itself informative and must be noted. Cite the specific file, line, or pattern that provides each answer.

### Functionality & Features
- What is the stated purpose of this project? (README, top-level docs, or inferred)
- What can this software *do*? List every distinct capability, feature, or operation end-to-end.
- What does it take as input and what does it produce as output?
- Are there distinct modes of operation or runtime configurations?
- What is fully implemented vs partially implemented vs stubbed? (Look for `todo!()`, `unimplemented!()`, TODOs)
- Are there any experimental, debug-only, or feature-flagged capabilities?
- What are the most important or complex features from an implementation standpoint?
- What edge cases or error conditions are explicitly handled?
- Are there any features that appear abandoned? (Commented-out code, dead functions, orphaned types)

### Architecture & Entry Points
- What is the entry point (`main`, `lib.rs`, workspace root)? What does it do?
- What is the top-level architectural pattern?
- If a workspace: all crates, their responsibilities, and inter-dependencies?
- What is the primary data flow end-to-end?
- Are there `build.rs` scripts or proc macros? What do they do?

### Module & Crate Boundaries
- Full module tree — every `mod` declaration
- What is `pub` vs `pub(crate)` vs private at each boundary, and why?
- Feature flags — all features and what they gate

### Design Decisions
- Why was this architecture chosen?
- All `// TODO`, `// FIXME`, `// HACK`, `// NOTE` comments — quote verbatim
- Commented-out code — what abandoned approaches do they suggest?
- Surprising or non-idiomatic choices — why?

### Engineering — Data Structures
- ALL structs and enums: purpose, fields/variants, derives, invariants
- Newtype wrappers — what do they enforce?
- `Arc`, `Rc`, `Cell`, `RefCell`, `Mutex` usage — what does each wrap and why?
- Self-referential types or non-trivial lifetimes

### Engineering — Functions
- Longest/most complex functions — what do they do?
- All public API functions
- Every `clone()` — intentional or borrow checker workaround?
- Every `unwrap()`/`expect()` — justified by what invariant?
- Generic functions — what do bounds express?

### Trait Architecture
- All custom traits — what does each abstract?
- `dyn Trait` vs `impl Trait` usage — where and why?
- Manual standard trait impls (`Display`, `From`, `Iterator`, `Drop`, etc.)

### Error Handling
- Error types defined — `thiserror`, `anyhow`, custom?
- Top-level `Result` alias?
- Propagation patterns and panic policy

### Concurrency & Async
- Async runtime — what and how configured?
- All `async fn`s and what they await
- `Arc<Mutex<>>` / channels — what do they protect and why?
- All `unsafe` blocks — quote and justify each

### Rust Patterns
- `Cow`, `Box<dyn>`, `impl Trait` in return position — where and why?
- All macros used (built-in and third-party)
- Notable iterator chains
- Advanced lifetime patterns

### Testing
- Test strategy — unit, integration, doc tests
- Fixtures, helpers, custom assertions
- Property-based testing
- Coverage of critical paths

### Build & Dependencies
- All `Cargo.toml` dependencies — why each non-obvious one was chosen
- MSRV, patched/git deps
- All Cargo features

---

## PHASE 0 — FUNCTIONALITY INVENTORY

This phase must be completed first. It answers: **what does this software actually do?**

### 0.1 Project Purpose
One-paragraph summary of what this project is and what problem it solves.

### 0.2 Feature & Capability List
An exhaustive list of every capability. For each:
```
Feature: [name]
Description: [what it does]
Entry point: [function, CLI flag, API call, etc.]
Status: [fully implemented / partial / stubbed / abandoned]
Notes: [limitations, TODOs, known issues]
```

### 0.3 Operational Modes
```
Mode: [name]
Trigger: [how entered]
Behavior differences:
```

### 0.4 Data & I/O
- Input formats and sources
- Output formats and destinations
- External systems / integrations
- What persists between runs

### 0.5 Implemented Algorithms & Logic
```
Algorithm/Logic: [name or description]
Location: [file/function]
Purpose:
Complexity/notes:
```

### 0.6 Unimplemented / Abandoned
- All `todo!()`, `unimplemented!()`, `// TODO` — quoted verbatim
- Dead code paths or orphaned types
- Incomplete features

---

## PHASE 1 — DESIGN KNOWLEDGE

### 1.1 Architecture
- Pattern + crate/module map
- End-to-end data flow
- Public API surface (if library)
- `build.rs` and proc macro effects

### 1.2 Design Decision Log
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

### 2.4 Error Handling Strategy
### 2.5 Concurrency & Async Design

---

## PHASE 3 — RUST PATTERNS

### 3.1 Ownership & Borrowing
### 3.2 Error & Option Idioms
### 3.3 Macro Usage
```
Macro:
Source:
Purpose:
⚠️ Non-obvious:
```
### 3.4 Build System & Feature Flags
### 3.5 Testing Strategy

---

## OUTPUT DOCUMENTS

**`FUNCTIONALITY.md`** ← produced first
```
# [Project] — Functionality Inventory
## Purpose
## Feature & Capability List
## Operational Modes
## Data & I/O
## Implemented Algorithms
## Unimplemented / Abandoned
```

**`DESIGN_KNOWLEDGE.md`**
```
# [Project] — Design Knowledge
## Architecture
## Public API Surface
## Design Decision Log
```

**`ENGINEERING_KNOWLEDGE.md`**
```
# [Project] — Engineering Knowledge
## Data Structures
## Function Contracts
## Trait Architecture
## Error Handling
## Concurrency & Async
## Rust Patterns
```

---

## FINAL NOTES FOR CLAUDE
- Produce `FUNCTIONALITY.md` first — it frames everything else
- Work through EVERY question in the Investigative Protocol before writing
- Cite file and line references for non-obvious findings
- Quote TODO/FIXME/HACK/NOTE/todo!()/unimplemented!() verbatim
- Flag every `unwrap()`, `unsafe`, `Arc<Mutex<>>` with a documented justification
- If a question has no answer, write "not found" — never skip
- Flag anything surprising or non-idiomatic with ⚠️
- The goal is: a developer with zero prior context can recreate this project from these docs alone
