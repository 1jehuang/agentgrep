# agentgrep

CLI-first code search and retrieval for agents.

## What it is

agentgrep is a new search tool design aimed at replacing the agent's first noisy burst of:

- `rg`
- `glob`
- `read`
- repeated follow-up probing

with a single query that returns a compact, structured, token-efficient result packet.

The core idea is:

> one broad investigative query in, one investigation-ready answer packet out.

agentgrep is designed to combine:

- `rg`-like trust and exactness in `grep`
- strong file discovery in `find`
- relation-aware, structure-aware investigation in `smart`

without requiring:

- a daemon
- heavyweight semantic indexing
- background memory usage
- opaque personalized ranking

## Current status

This repository is currently in the **architecture and planning** phase.

Key docs:

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- [`docs/INTERFACE.md`](docs/INTERFACE.md)
- [`docs/SMART_OUTPUT.md`](docs/SMART_OUTPUT.md)
- [`docs/QUERY_INTENT.md`](docs/QUERY_INTENT.md)
- [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md)
- [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md)

## Product thesis

agentgrep should be:

- **`rg`-trustworthy in exact mode**
- **better than file-list grep for discovery**
- **good enough in smart mode that the first query often replaces several manual search/read steps**
- **token-efficient in the results it returns**

The key observation is that agents often search with questions like:

- where is `X` called from
- where is `X` rendered
- where is `X` populated
- where does `X` come from
- what handles `X`
- how is `X` wired up

The goal is not to make those queries magically semantic.
The goal is to internally perform the broad lexical probing the agent would otherwise do manually, then return the smallest useful context packet.

For reliability, `smart` is expected to use a small structured DSL rather than freeform natural language.

## Planned commands

```bash
agentgrep grep <query> [--path <root>]
agentgrep find <query parts...> [--path <root>]
agentgrep smart subject:<value> relation:<value> [support:<value> ...] [kind:<value>] [path:<hint>] [--path <root>]
```

## Principles

- exact search is sacred
- smartness is layered
- results should minimize follow-up reads
- cache is optional
- semantic search, if ever added, is a sidecar rather than the foundation
- one-shot CLI behavior is the default model

## Why this exists

Existing tools force a tradeoff between:

- fast exact CLI search
- fuzzy file finding
- semantic retrieval
- structured agent-oriented output

agentgrep is trying to close that gap with a lexical-first, structure-aware design tuned specifically for agent investigation workflows.
