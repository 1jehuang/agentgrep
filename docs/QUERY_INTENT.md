# agentgrep Query Intent Spec

## Purpose

This document defines how broad agentgrep queries should be interpreted without requiring embeddings.

The goal is not deep semantic understanding.
The goal is to turn one broad user query into an internal multi-pass lexical search plan.

## Core principle

A query should normalize into a small structured object with:

- raw text
- normalized text
- likely subject
- likely relation/intent
- support terms
- mode (`grep`, `find`, `smart`)

## Query object

```rust
struct ParsedQuery {
    raw: String,
    normalized_text: String,
    tokens: Vec<String>,
    identifier_tokens: Vec<String>,
    path_tokens: Vec<String>,
    phrase_tokens: Vec<String>,
    mode: QueryMode,
    intent: QueryIntent,
    subject: Option<String>,
    relation: Option<RelationKind>,
    support_terms: Vec<String>,
    constraints: QueryConstraints,
}
```

## Query modes

### `grep`
User wants exact lexical truth.

### `find`
User wants files/paths.

### `smart`
User wants interpretation, multi-pass probing, fusion, and grouped output.

## Intent taxonomy

### `definition`
Examples:
- `where is AuthStatus defined`
- `what is build_auth_status_line`
- `find the implementation of auth_status`

Boost:
- definitions
- type declarations
- function definitions
- primary implementation files

### `called_from`
Examples:
- `where is auth_status called from`
- `who calls check_selfdev_signals`

Boost:
- call expressions
- executable references
- dispatch and entrypoint code

Penalize:
- definition lines
- imports/reexports

### `triggered_from`
Examples:
- `where is compaction triggered from`
- `what triggers reconnect_attempts`

Boost:
- event loops
- dispatch logic
- callbacks
- scheduling/queueing code

### `rendered`
Examples:
- `where is auth_status rendered`
- `where is the memory widget rendered`
- `where is header label drawn`

Expand relation terms with:
- render
- draw
- widget
- view
- layout
- ui
- panel
- component

Boost:
- UI files/modules
- render-like functions
- widget/component construction

### `populated`
Examples:
- `where is remote_available_models populated`
- `where is image_regions set`

Expand relation terms with:
- populate
- set
- assign
- insert
- push
- append
- build
- init
- update

Boost:
- assignment expressions
- field writes
- builder assembly
- initialization code

### `comes_from`
Examples:
- `where does provider_name come from`
- `where does width come from`

Expand relation terms with:
- source
- load
- fetch
- read
- parse
- deserialize
- initialize
- derive

Boost:
- upstream providers
- config loading
- parsing/deserialization
- constructor paths

### `handled`
Examples:
- `what handles scroll events`
- `where is paste handled`
- `what handles subscribe response`

Expand relation terms with:
- handle
- handler
- dispatch
- route
- on_
- callback
- event

Boost:
- `handle_*` functions
- match/routing code
- callbacks
- event loop logic

## Processing model without embeddings

For a query like:

```text
where is auth_status rendered
```

agentgrep should:

1. keep the raw query
2. normalize it
3. extract `auth_status` as likely subject
4. extract `rendered` as relation hint
5. expand into lexical probes
6. run internal searches for:
   - subject matches
   - relation-context matches
   - file-role/path matches
   - co-occurrence evidence
7. rerank and group results

## Important boundary

This design should work well for:
- identifier + relation queries
- mixed natural-language + symbol queries
- repo navigation questions with strong lexical anchors

It is not trying to fully solve weak-anchor conceptual queries in v1.
