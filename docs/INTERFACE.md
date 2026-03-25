# agentgrep Interface

## Design goal

The interface surface should be **small, opinionated, and composable**.

agentgrep should sit between:

- `rg`: one powerful search primitive with many knobs
- MCP-style retrieval systems: many specialized operations

The v1 sweet spot is a small number of strong commands with rich result shapes.

## Primary commands

```bash
agentgrep grep <query>
agentgrep find <query>
agentgrep smart <query>
```

## Command semantics

### `agentgrep grep`
Exact lexical search.

Use when the user wants:

- exact content matches
- predictable semantics
- exhaustive results
- script-friendly behavior

Examples:

```bash
agentgrep grep auth_status
agentgrep grep --regex "auth_.*status"
agentgrep grep --type rs auth_status
agentgrep grep --json auth_status
```

### `agentgrep find`
Ranked file/path discovery.

Use when the user wants:

- likely files
- approximate path/name lookup
- identifier-form robustness
- better file discovery than `rg --files | rg`

Examples:

```bash
agentgrep find auth status
agentgrep find debug socket
agentgrep find provider mod
```

### `agentgrep smart`
Broad investigation mode using a small structured DSL.

Use when the user wants:

- one search that internally performs several lexical probes
- relation-aware results
- grouped files plus useful local structure
- the smallest useful context packet
- less ambiguity than freeform natural language

Examples:

```bash
agentgrep smart subject:auth_status relation:rendered
agentgrep smart subject:provider_name relation:comes_from support:config
agentgrep smart subject:scroll relation:handled support:event
agentgrep smart subject:lsp relation:implementation
```

## Shared flags

### Output

```bash
--json
--no-color
--paths-only
--debug-score
```

### Scope

```bash
--type <type>
--glob <glob>
--hidden
--no-ignore
--path <root>
```

### Result budgeting

```bash
--max-files <n>
--max-regions <n>
```

### Debug and trust

```bash
--debug-plan
--debug-score
--debug-structure
```

## Later possible commands

Not part of the initial surface, but plausible later if clearly useful:

```bash
agentgrep outline ...
agentgrep symbols ...
```

The project should resist surface-area growth until `grep`, `find`, and `smart` are excellent.

## Philosophy by mode

### `grep` = truth
- exact
- exhaustive
- predictable
- closer to `rg`

### `find` = discovery
- file-oriented
- ranked
- approximate and robust

### `smart` = investigation
- grouped
- structured
- relation-aware
- optimized for fewer follow-up reads
