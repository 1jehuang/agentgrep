# agentgrep

CLI-first code search and retrieval for agents.

`agentgrep` is a small Rust CLI that tries to replace an agent's first noisy burst of:

- `rg`
- file listing / globbing
- repeated `read` calls
- ad-hoc follow-up probing

with one search command that returns a **compact, structured, investigation-ready result packet**.

It has four modes:

- **`grep`** — exact lexical search, grouped by file and enclosing symbol
- **`find`** — ranked file discovery with structure summaries
- **`outline`** — known-file structural scan without reading the whole body
- **`trace`** — structured investigation with ranked files **and** ranked code regions

The goal is not “semantic magic.” The goal is to do the broad lexical probing an agent would otherwise do manually, then return the **smallest useful context packet**.

## Why this exists

Classic CLI search tools are excellent at exact matching, but agents often need more than exact matching.

Typical agent questions look like:

- where is `X` rendered?
- where is `X` implemented?
- what handles `X`?
- where does `X` come from?
- which files are probably relevant to this topic?

`agentgrep` is built for that workflow while staying:

- **CLI-first**
- **one-shot**
- **lexical-first**
- **transparent**
- **scriptable**
- **daemon-free**
- **index-free**

## Status

`agentgrep` is implemented and usable today.

Current commands:

- `agentgrep grep`
- `agentgrep find`
- `agentgrep outline`
- `agentgrep trace` (with `smart` kept as an alias)

Current properties:

- no daemon
- no background index
- no embeddings
- no hidden personalization
- optional harness-provided retrieval context for smarter result shaping
- plain text output by default
- JSON output for automation

## Install

### Build locally

```bash
cargo build --release
./target/release/agentgrep --help
```

### Install from the repo checkout

```bash
cargo install --path .
```

## Quick start

### 1. Exact search: `grep`

Use `grep` when you know the text or symbol and want exact, exhaustive matches.

```bash
agentgrep grep auth_status
agentgrep grep --regex 'auth_.*status'
agentgrep grep --type rs auth_status
agentgrep grep --path /path/to/repo auth_status
```

Side-by-side comparison with `rg`:

```text
rg -n auth_status src/auth/mod.rs
src/auth/mod.rs:218:pub fn auth_status() -> AuthStatus
src/auth/mod.rs:241:let status = auth_status();

agentgrep grep auth_status --path /repo
query: auth_status
matches: 6 in 3 files

src/auth/mod.rs
  symbols: 4 total, 1 matched, 3 other
    - function auth_status @ 218-246
      - @ 218 pub fn auth_status() -> AuthStatus
      - @ 241 let status = auth_status();
    - other: enum AuthStatus @ 180-210; function format_status @ 247-268; impl AuthStatus @ 269-320
```

`rg` optimizes for raw match streaming. `agentgrep grep` keeps exact semantics, but returns a more agent-ready packet:

- grouped by file
- grouped by enclosing symbol when possible
- preserves exact matching lines
- includes a compact hint of other structure in the file

### 2. Ranked file discovery: `find`

Use `find` when you know the topic but not the exact symbol.

```bash
agentgrep find auth status
agentgrep find debug socket
agentgrep find transcription transcript voice dictate speech input
agentgrep find --type rs remote session metadata
```

Output shape:

```text
query: auth status
top files: 5

1. src/auth/mod.rs
   role: auth
   why:
     - path token matches: 2
     - symbol/outline hits: 3
   structure:
     - enum AuthStatus @ 180-210 (31 lines)
     - function auth_status @ 218-246 (29 lines)
     ... 5 more symbols
```

### 3. File structure scan: `outline`

Use `outline` when you already know the file and want its structure without fully reading it.

```bash
agentgrep outline src/tool/lsp.rs
agentgrep outline --path /path/to/repo src/tui/app/remote.rs
agentgrep outline --max-items 20 src/main.rs
```

Output shape:

```text
file: src/tool/lsp.rs
language: rust
role: implementation
lines: 95
symbols: 9

structure:
  - struct LspTool @ 20-21 (2 lines)
  - impl LspTool @ 22-22 (1 lines)
  - struct LspInput @ 29-37 (9 lines)
  - impl Tool @ 38-38 (1 lines)
  - function name @ 39-42 (4 lines)
  - function description @ 43-47 (5 lines)
  - function parameters_schema @ 48-75 (28 lines)
  - function execute @ 76-95 (20 lines)
```

### 4. Relation-aware tracing: `trace`

Use `trace` when the question is about a **relationship**, not just a string.

```bash
agentgrep trace subject:auth_status relation:rendered
agentgrep trace subject:lsp relation:implementation kind:code path:src/tool
agentgrep trace subject:provider_name relation:comes_from support:config
agentgrep trace subject:scroll relation:handled support:event
```

Output shape:

```text
query parameters:
  subject: auth_status
  relation: rendered
  kind: code
  path_hint: src/tui

top results: 3 files, 4 regions
best answer likely in src/tui/app.rs

1. src/tui/app.rs
   role: ui
   why:
     - exact subject match or symbol hit
     - relation-context hits: 2
   structure:
     - function render_status_bar @ 9002-9017 (16 lines)
     - function draw_header @ 9035-9056 (22 lines)
     ... 6 more symbols
   regions:
     - render_status_bar @ 9002-9017 (16 lines)
       kind: render-site
       full region:
         fn render_status_bar(&self, ui: &mut Ui) {
             let status = auth_status();
             ui.label(status.to_string());
         }
       why:
         - exact subject match
         - relation-context aligned
```

## When to use which mode

- **Use `grep`** when you need exact matches.
- **Use `find`** when you want the best files to inspect next.
- **Use `outline`** when you know the file and want the structure first.
- **Use `trace`** when you want the likely answer region for a relation-aware question.

A simple heuristic:

- exact string → `grep`
- topic / subsystem → `find`
- known file, no body yet → `outline`
- relation / usage / origin / handling → `trace`

## Output format by mode

### `grep`

```text
query: <literal-or-regex>
matches: <match_count> in <file_count> files

<file>
  symbols: <total> total, <matched> matched, <other> other
    - <kind> <label> @ <start>-<end>
      - @ <line> <exact matching line>
    - <file scope>
      - @ <line> <exact matching line>
    - other: <kind> <label> @ <start>-<end>; ...
```

### `find`

```text
query: <topic query>
top files: <count>

1. <file>
   role: <role>
   why:
     - <reason>
   structure:
     - <kind> <label> @ <start>-<end> (<line_count> lines)
     ... <n> more symbols
```

### `outline`

```text
file: <file>
language: <language>
role: <role>
lines: <line_count>
symbols: <symbol_count>

structure:
  - <kind> <label> @ <start>-<end> (<line_count> lines)
  ... <n> more symbols
context: <optional harness-context note>
```

### `trace`

```text
query parameters:
  subject: <subject>
  relation: <relation>
  support: <optional support terms>
  kind: <optional kind>
  path_hint: <optional subtree hint>

top results: <file_count> files, <region_count> regions
best answer likely in <file>

1. <file>
   role: <role>
   why:
     - <reason>
   structure:
     - <kind> <label> @ <start>-<end> (<line_count> lines)
   regions:
     - <label> @ <start>-<end> (<line_count> lines)
       kind: <region_kind>
       snippet: | full region:
         <body>
       why:
         - <reason>
   context: <optional harness-context note>
```

## Smart query DSL

`trace` uses a small, explicit DSL instead of freeform natural language.

Required:

- `subject:<value>`
- `relation:<value>`

Optional:

- `support:<value>` (repeatable)
- `kind:<code|docs|tests|...>`
- `path:<subtree-hint>`

Examples:

```bash
agentgrep trace subject:debug_socket relation:defined kind:code path:src
agentgrep trace subject:TranscriptMode relation:implementation kind:code path:src/tui
agentgrep trace subject:provider_name relation:comes_from support:config
```

Built-in relation aliases include:

- `defined`
- `called_from`
- `triggered_from`
- `rendered`
- `populated`
- `comes_from`
- `handled`
- `implementation`

### Harness-aware retrieval context

`outline` and `trace` can optionally accept a harness-provided context packet:

```bash
agentgrep outline --context-json /tmp/agentgrep-context.json src/tool/lsp.rs
agentgrep trace --context-json /tmp/agentgrep-context.json subject:lsp relation:implementation kind:code path:src/tool
```

This lets a harness tell `agentgrep` what the agent already likely knows, so it can:

- compress repeated outlines
- avoid re-inlining unchanged regions
- prefer novel context when appropriate

See [docs/HARNESS_CONTEXT.md](docs/HARNESS_CONTEXT.md) for the contract and implementation guidance.

## Output modes

All three commands support script-friendly output forms.

### JSON

```bash
agentgrep grep --json auth_status
agentgrep find --json debug socket
agentgrep trace --json subject:lsp relation:implementation kind:code
```

### Paths only

```bash
agentgrep grep --paths-only auth_status
agentgrep find --paths-only debug socket
agentgrep trace --paths-only subject:lsp relation:implementation
```

## Region expansion in `trace`

`trace` supports:

```bash
--full-region auto
--full-region always
--full-region never
```

Current behavior:

- `always` → always inline the full region
- `never` → always show a focused snippet
- `auto` → inline the full region when the matched structural unit is small enough to be cheap

Today, `auto` is intentionally conservative.

## Benchmark snapshot

Benchmarks below were run on the `jcode` repo using the release build on:

- Linux 6.18
- Intel Core Ultra 7 256V
- no daemon
- no index
- warm runs via `hyperfine`

### Raw latency

| Case | Command | Mean time |
| --- | --- | ---: |
| Exact literal search | `agentgrep grep --path /home/jeremy/jcode transcription` | **30.9 ms** |
| Exact literal baseline | `rg -n transcription /home/jeremy/jcode` | **8.2 ms** |
| Exact regex search | `agentgrep grep --regex --path /home/jeremy/jcode 'transcript|voice|dictation|speech'` | **44.2 ms** |
| Exact regex baseline | `rg -n -e 'transcript|voice|dictation|speech' /home/jeremy/jcode` | **8.7 ms** |
| Ranked file discovery | `agentgrep find --path /home/jeremy/jcode transcription transcript voice dictate speech input message` | **6.1 ms** |
| Structured tracing | `agentgrep trace --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui` | **17.3 ms** |

### What those numbers mean

- `grep` is **not trying to beat `rg` on raw speed**. `rg` is the baseline for exact search performance.
- The current `find` and `trace` implementation benefits a lot from metadata-first filtering before reading/parsing files.
- The interesting tradeoff is whether one `find` or `trace` query saves several follow-up `grep` + `read` steps.

For a representative `jcode` query, rough human-readable output sizes were:

- `grep`: 23 lines / 687 bytes
- `find`: 81 lines / 2754 bytes
- `trace`: 191 lines / 6961 bytes

See [docs/BENCHMARKS.md](docs/BENCHMARKS.md) for commands and reproduction details.

## Limitations

Current limitations are intentional:

- `grep` is still slower than `rg`
- `trace` uses a small DSL rather than full natural language
- `trace` region expansion is still conservative
- there is no persistent index yet
- ranking is lexical/structural, not embedding-based

## Scripts

- `scripts/smoke.sh <repo-path>` — quick manual smoke test
- `scripts/benchmark.sh <repo-path>` — reproducible benchmark run

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- [docs/INTERFACE.md](docs/INTERFACE.md)
- [docs/HARNESS_CONTEXT.md](docs/HARNESS_CONTEXT.md)
- [docs/SMART_OUTPUT.md](docs/SMART_OUTPUT.md)
- [docs/QUERY_INTENT.md](docs/QUERY_INTENT.md)
- [docs/BENCHMARKS.md](docs/BENCHMARKS.md)
- [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)

## Development

Run tests:

```bash
cargo test
```

Run a smoke test against another repo:

```bash
scripts/smoke.sh /path/to/repo
```

Run benchmarks:

```bash
scripts/benchmark.sh /path/to/repo
```

## License

MIT
