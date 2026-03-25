# agentgrep Architecture

## Product thesis

**agentgrep** is a **CLI-first code finder for agents** built around a simple idea:

> keep exact search trustworthy, let the tool internally perform multi-pass lexical probing, and return grouped, structure-aware, token-efficient results.

The primary job of agentgrep is not just to search text.
It is to replace the agent's first burst of `rg`/`glob`/`read` calls with one better retrieval step.

## Primary success criteria

agentgrep wins if developers and agents reach for it instead of a mix of:

- `rg` for exact text
- `rg --files | rg` or `fd` for file finding
- repeated repo probing to understand structure
- heavier semantic tools for early-stage codebase navigation

### v1 success criteria

1. **Exact mode is excellent**
   - fast
   - exhaustive
   - deterministic
   - trustworthy
2. **Find mode is excellent**
   - robust to approximate path/name queries
   - ranked well
   - cold-path usable
3. **Smart mode is genuinely investigation-saving**
   - one broad query often replaces several manual searches
   - grouped results reduce follow-up reads
   - outputs are explainable
4. **Operational overhead is tiny**
   - one binary
   - no daemon
   - no model runtime
   - lightweight cache only

## Hard architectural invariants

### 1. One-shot first
Every command must be correct and useful in a fresh process.

```text
parse -> search -> rank -> render -> exit
```

### 2. Exact search is sacred
`agentgrep grep` must preserve grep-like expectations:

- exact lexical matching only by default
- exhaustive within the selected scope
- deterministic
- no fuzzy matching by default
- no semantic interpretation by default

Smartness may improve packaging and optional presentation, but it must not corrupt exact semantics.

### 3. Smartness is layered
Heuristic behavior must happen after cheap, bounded candidate generation.
The tool may do multiple internal search passes, but those passes should remain lexical-first and inspectable.

### 4. Cache is optional
If cache is missing, stale, corrupted, or disabled, agentgrep still works.

### 5. Semantic is a sidecar
If semantic retrieval is ever added, it is optional and additive.
It is never the default foundation.

## Core commands

```bash
agentgrep grep <query>
agentgrep find <query>
agentgrep smart <query>
```

### `grep`
Purpose: exact content search.

### `find`
Purpose: ranked file/path discovery.

### `smart`
Purpose: one broad investigative query in, one grouped answer packet out.

## High-level system design

```text
CLI
  -> Query parser
  -> Query normalizer
  -> Query planner
  -> Candidate generation
       -> exact subject search
       -> relation/context search
       -> path/file-role search
       -> co-occurrence search
  -> Region and structure extraction
  -> Reranker / fusion
  -> Renderer

Optional acceleration layers
  -> Tier 1: manifest cache
  -> Tier 2: sparse lexical sidecar
  -> Tier 3: optional semantic sidecar
```

## Key design choice: internal multi-pass search

The user may ask a broad query like:

```text
where is auth_status rendered
```

agentgrep should not rely on deep semantic understanding to answer this.
Instead, it should:

1. extract likely anchors (`auth_status`)
2. extract relation hints (`rendered`)
3. expand into lexical probes (`render`, `draw`, `ui`, `widget`, etc.)
4. search for subject, relation context, path/file-role evidence, and co-occurrence
5. return grouped results with structural context

In other words:

> the user asks one broad query, the engine internally performs the blast-rg workflow, and the output is a compressed investigation packet.

## Result-oriented architecture

agentgrep should optimize both:

1. **query efficiency**
   - fewer search tool calls
2. **context efficiency**
   - fewer follow-up read tokens

That means ranking is not just about relevance.
It is also about:

- minimizing follow-up reads
- surfacing self-contained answer regions
- showing just enough structure for the next decision

## Document-aware structure

v1 is code-first, but the architecture should support document structure more generally.

For any result-bearing document, the engine should eventually be able to return:

- path
- document kind
- compact structure summary
- relevant regions
- per-region metadata

This generalizes naturally to:

- code
- logs
- config
- markdown/docs

But v1 should optimize for code first.

## Structural emphasis before semantics

Before embeddings, agentgrep should get smarter via:

- identifier normalization
- relation-aware query parsing
- file-role hints
- structure extraction
- region-kind classification
- nearby-match merging
- grouped output

This is the intended path to strong quality without semantic-system overhead.
