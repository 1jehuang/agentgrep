# agentgrep Harness Context Contract

## Purpose

This document describes how an agent harness can provide **retrieval context** to `agentgrep` so that result shaping becomes more intelligent without making `agentgrep` tightly coupled to any one runtime.

The goal is not to make `agentgrep` parse a whole chat transcript.
The goal is to let a harness supply a **clean, abstract view of what the agent likely already knows**.

This is especially useful for:

- avoiding repeated full-region returns
- reducing redundant structure output
- preferring novel files or regions
- re-expanding content that has changed since the agent last saw it
- supporting long-context and multi-agent runtimes without baking their behavior into `agentgrep`

## Core design principle

`agentgrep` should consume:

> **retrieval-state hints**

not:

> raw prompt mechanics, raw transcript dumps, or harness-specific memory internals

That means the harness should do the messy work of observing tool outputs, transcript deltas, and model-context behavior, then emit a much smaller, clearer state packet.

## Why the harness should own this logic

The harness knows things that `agentgrep` should not have to know:

- the model's context window size
- prompt compaction/summarization behavior
- which turns are still strongly represented in-context
- which files or snippets were exposed via non-`read` channels
- which outputs were shown by `bash`, pasted by the user, or surfaced by other agents
- how aggressively the runtime should prune repeated information

If `agentgrep` tries to infer all of that itself from raw chat or tool deltas, it becomes:

- harness-specific
- brittle to transcript formatting changes
- harder to test
- harder to reuse outside one environment

A better split is:

- **harness** computes familiarity / freshness / pruning hints
- **agentgrep** uses those hints to shape results

## What the harness should track internally

The harness may internally track very rich state.
For example:

- `read` calls
- `bash` outputs that exposed file content
- `agentgrep` outputs
- `grep` / `git show` / `git diff` outputs
- user-pasted code snippets
- side-panel file loads
- shared swarm context from other agents
- prompt compaction position and retention heuristics

However, that internal state should be compiled down into a much smaller external contract before being passed to `agentgrep`.

## The right abstraction: confidence, not timestamps

A raw field like:

```json
{ "last_seen_at": "2026-03-27T00:20:00Z" }
```

is usually a weak abstraction.
Wall-clock time is not the main question.
What matters is more like:

- is this likely still represented in-context?
- was it seen as structure only, or as full body?
- has the underlying file changed since then?
- how safe is it to prune repeated detail?

So the harness should usually provide **confidence-style signals** instead of raw time semantics.

## Recommended context fields

### 1. `structure_confidence`

How confident is the harness that the agent already knows the **shape** of a file or symbol?

Example:

```json
{
  "path": "src/tool/lsp.rs",
  "structure_confidence": 0.95
}
```

#### What it means

High `structure_confidence` suggests the agent probably does not need:

- the full file outline repeated
- long lists of symbols already seen
- redundant structure-only context

#### When a harness should raise it

- the agent saw an `outline` result
- the agent read a substantial portion of the file
- the file's symbol layout was shown in a prior `find` / `trace` result
- the file remains strongly represented in active context

#### How `agentgrep` should use it

- compress repeated structure sections
- prefer only directly relevant symbols
- spend more budget on novel or body-level information

---

### 2. `body_confidence`

How confident is the harness that the agent already knows the **body/content** of a region or symbol?

Example:

```json
{
  "path": "src/tool/lsp.rs",
  "start_line": 76,
  "end_line": 95,
  "body_confidence": 0.80
}
```

#### Why this matters

Knowing the file structure is not the same as knowing the implementation body.
An agent might know a symbol exists without remembering its full contents.

#### When a harness should raise it

- the agent explicitly read that region
- the full region was returned by `trace`
- a `bash` command showed the full function or file
- the agent itself recently operated on or edited that exact region

#### How `agentgrep` should use it

- high confidence + unchanged content → header/range may be enough
- low confidence → return more body/snippet detail

---

### 3. `current_version_confidence`

How confident is the harness that the agent's remembered content still matches the **current file contents**?

Example:

```json
{
  "path": "src/tool/lsp.rs",
  "start_line": 76,
  "end_line": 95,
  "current_version_confidence": 0.25
}
```

#### Why this matters

An agent may have seen a region before, but if the file changed, the old familiarity should not cause aggressive pruning.

#### Good ways for a harness to compute it

- compare file hashes / mtimes / content signatures
- track whether the region or file changed since exposure
- invalidate confidence when edits overlap the region

#### How `agentgrep` should use it

- low current-version confidence → re-expand detail
- high current-version confidence → stronger pruning is safe

---

### 4. `prune_confidence`

How safe is it to aggressively compress repeated detail for this target?

Example:

```json
{
  "path": "src/tool/lsp.rs",
  "symbol": "execute",
  "prune_confidence": 0.88
}
```

#### Why this is useful

This is the most direct hint for `agentgrep`.
It lets the harness combine many internal factors into one actionable signal.

#### Example internal factors a harness might use

- exposure strength was high
- content is unchanged
- agent is still likely to remember it
- the symbol/file is in active focus
- repeated expansion would waste tokens

#### How `agentgrep` should use it

- high → header/range only
- medium → short snippet
- low → fuller body / richer structure

---

### 5. `source_strength`

A categorical description of how directly the agent saw something.

Suggested values:

- `outline_only`
- `match_line_only`
- `snippet`
- `full_region`
- `full_file`
- `summary_only`

Example:

```json
{
  "path": "src/tool/lsp.rs",
  "start_line": 76,
  "end_line": 95,
  "source_strength": "full_region"
}
```

#### Why it matters

Not all exposure is equally strong.
A `grep` hit is much weaker than a full-body `read`.

#### How `agentgrep` should use it

- `outline_only` → structure pruning okay, body pruning not okay
- `match_line_only` → avoid repeating the exact same line, but not the whole symbol
- `full_region` / `full_file` → stronger body-level pruning is allowed

---

### 6. `focus` or `focus_files`

Files the harness believes are currently central to the agent's task.

Example:

```json
{
  "focus_files": [
    "src/tui/app/remote.rs",
    "src/tui/ui_messages.rs"
  ]
}
```

#### Why it matters

Focus often changes how much overview is needed.
If the agent is already actively working in a file, broad file orientation is less useful than targeted deltas or exact relevant regions.

#### How `agentgrep` should use it

- reduce broad orientation for already-focused files
- emphasize novel or changed regions within them

---

### 7. `reasons`

Optional reason codes or short explanations that explain why the harness assigned a confidence.

Example:

```json
{
  "prune_confidence": 0.88,
  "reasons": [
    "recent_full_region_exposure",
    "file_unchanged",
    "still_in_active_context"
  ]
}
```

#### Why this is useful

It makes the system debuggable.
Harness authors and `agentgrep` authors can reason about behavior without exposing raw harness internals.

## Recommended contract shape

A harness-facing integration could expose something like:

```json
{
  "version": 1,
  "known_regions": [
    {
      "path": "src/tool/lsp.rs",
      "start_line": 76,
      "end_line": 95,
      "structure_confidence": 0.95,
      "body_confidence": 0.80,
      "current_version_confidence": 0.90,
      "prune_confidence": 0.85,
      "source_strength": "full_region",
      "reasons": ["recent_full_region_exposure", "file_unchanged"]
    }
  ],
  "known_files": [
    {
      "path": "src/tool/lsp.rs",
      "structure_confidence": 0.95,
      "body_confidence": 0.60,
      "current_version_confidence": 0.90,
      "prune_confidence": 0.70,
      "source_strength": "full_file"
    }
  ],
  "known_symbols": [
    {
      "path": "src/tool/lsp.rs",
      "symbol": "execute",
      "kind": "function",
      "structure_confidence": 0.95,
      "body_confidence": 0.75,
      "current_version_confidence": 0.90,
      "prune_confidence": 0.82
    }
  ],
  "focus_files": [
    "src/tool/lsp.rs"
  ]
}
```

This is only a suggested shape.
The deeper design point is the abstraction, not the exact field names.

## What harnesses should usually *not* expose directly

Harnesses often have internal signals like:

- token distance from the prompt head
- message turn index
- context-window position
- compaction stage
- raw transcript chunks
- raw wall-clock timestamps

Those are useful **internally**, but they are usually poor API fields for `agentgrep`.

Instead, the harness should convert them into normalized retrieval hints like:

- `structure_confidence`
- `body_confidence`
- `current_version_confidence`
- `prune_confidence`

This keeps the contract more portable across runtimes.

## How harnesses should compute these fields well

### A. Distinguish structure knowledge from body knowledge

This is one of the most important implementation details.

A harness should avoid treating these as the same thing:

- agent saw a symbol list
- agent saw exact lines from inside the function
- agent saw the whole function body
- agent saw the whole file

Those should usually produce different confidence profiles.

### B. Prefer confidence over timestamps

A low-level timestamp may be one input into harness logic, but it should not be the final signal.

A better harness will combine:

- exposure strength
- prompt/context location
- summary/compaction state
- file change status
- current task focus
- model/runtime behavior

into a higher-level confidence value.

### C. Invalidate aggressively when files change

A harness should lower `current_version_confidence` when:

- the file hash changes
- the relevant region changes
- another agent edits the file
- the active branch/worktree changed in a way that invalidates exposure

### D. Be conservative when uncertain

If the harness is unsure whether the agent truly saw or retained a region, it should lower confidence instead of over-claiming knowledge.

Over-pruning is usually worse than modest repetition.

### E. Track exposure from more than `read`

A good harness should not assume the agent only sees code through a `read` tool.
It should attempt to capture exposure from:

- `read`
- `bash` output (`cat`, `sed`, `git show`, etc.)
- prior `agentgrep` results
- user-pasted code
- side-panel file loads
- shared agent summaries where appropriate

### F. Consider swarm/shared context carefully

In multi-agent systems, another agent may have already inspected a region deeply.
That can justify lighter repetition at the swarm level, but it does **not** automatically mean the local agent knows it.

A harness should distinguish:

- locally known
- swarm-known but not locally known

## How `agentgrep` should use the contract

Given the context packet, `agentgrep` can shape output like this:

### High structure confidence, low body confidence

Likely policy:
- compress structure section
- return body or snippet for the relevant symbol

### High body confidence, high current-version confidence, high prune confidence

Likely policy:
- return header + range
- avoid re-inlining the full body

### Low current-version confidence

Likely policy:
- re-expand detail
- show more of the current body
- ignore aggressive pruning

### Low confidence across the board

Likely policy:
- be verbose enough to stand on its own
- richer structure and body-level context are appropriate

## Minimum viable harness implementation

A very good first version only needs to do a few things:

1. track exposed files/regions
2. separate structure exposure from body exposure
3. detect whether files changed since exposure
4. output conservative confidence estimates

That alone can significantly improve retrieval shaping.

## Future extensions

Possible future context fields include:

- novelty preference
- local-vs-swarm familiarity
- explicit return policy hints
- confidence per symbol kind
- file-role familiarity
- changed-region maps instead of file-level freshness only

## Summary

Harness authors should think of their job as:

> compile noisy runtime history into a compact retrieval-state packet

The most important outcome is not perfect memory simulation.
It is giving `agentgrep` a trustworthy, portable answer to questions like:

- does the agent already know the structure here?
- does the agent already know the body here?
- is that knowledge still current?
- how safe is it to prune repeated detail?

If harnesses provide those signals well, `agentgrep` can become much better at returning the right amount of context without coupling itself to one specific agent runtime.
