# agentgrep

CLI-first code search and retrieval for agents.

`agentgrep` is a small Rust CLI that tries to replace an agent's first noisy burst of:

- `rg`
- file listing / globbing
- repeated `read` calls
- ad-hoc follow-up probing

with one search command that returns a **compact, structured, investigation-ready result packet**.

It has three modes:

- **`grep`** — exact lexical search, grouped by file
- **`find`** — ranked file discovery with structure summaries
- **`smart`** — structured investigation with ranked files **and** ranked code regions

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
- `agentgrep smart`

Current properties:

- no daemon
- no background index
- no embeddings
- no hidden personalization
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

Example output:

```text
query: auth_status
matches: 6 in 3 files

src/auth/mod.rs
  matches:
    - @ 218
      pub fn auth_status() -> AuthStatus
    - @ 241
      let status = auth_status();
```

### 2. Ranked file discovery: `find`

Use `find` when you know the topic but not the exact symbol.

```bash
agentgrep find auth status
agentgrep find debug socket
agentgrep find transcription transcript voice dictate speech input
agentgrep find --type rs remote session metadata
```

Example output:

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

Example output:

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

### 4. Structured investigation: `smart`

Use `smart` when the question is about a **relationship**, not just a string.

```bash
agentgrep smart subject:auth_status relation:rendered
agentgrep smart subject:lsp relation:implementation kind:code path:src/tool
agentgrep smart subject:provider_name relation:comes_from support:config
agentgrep smart subject:scroll relation:handled support:event
```

Example output:

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
- **Use `smart`** when you want the likely answer region for a relation-aware question.

A simple heuristic:

- exact string → `grep`
- topic / subsystem → `find`
- known file, no body yet → `outline`
- relation / usage / origin / handling → `smart`

## Smart query DSL

`smart` uses a small, explicit DSL instead of freeform natural language.

Required:

- `subject:<value>`
- `relation:<value>`

Optional:

- `support:<value>` (repeatable)
- `kind:<code|docs|tests|...>`
- `path:<subtree-hint>`

Examples:

```bash
agentgrep smart subject:debug_socket relation:defined kind:code path:src
agentgrep smart subject:TranscriptMode relation:implementation kind:code path:src/tui
agentgrep smart subject:provider_name relation:comes_from support:config
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

## Output modes

All three commands support script-friendly output forms.

### JSON

```bash
agentgrep grep --json auth_status
agentgrep find --json debug socket
agentgrep smart --json subject:lsp relation:implementation kind:code
```

### Paths only

```bash
agentgrep grep --paths-only auth_status
agentgrep find --paths-only debug socket
agentgrep smart --paths-only subject:lsp relation:implementation
```

## Region expansion in `smart`

`smart` supports:

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
| Exact literal search | `agentgrep grep --path /home/jeremy/jcode transcription` | **37.9 ms** |
| Exact literal baseline | `rg -n transcription /home/jeremy/jcode` | **8.2 ms** |
| Exact regex search | `agentgrep grep --regex --path /home/jeremy/jcode 'transcript|voice|dictation|speech'` | **40.0 ms** |
| Exact regex baseline | `rg -n -e 'transcript|voice|dictation|speech' /home/jeremy/jcode` | **8.7 ms** |
| Ranked file discovery | `agentgrep find --path /home/jeremy/jcode transcription transcript voice dictate speech input message` | **6.1 ms** |
| Structured investigation | `agentgrep smart --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui` | **37.4 ms** |

### What those numbers mean

- `grep` is **not trying to beat `rg` on raw speed**. `rg` is the baseline for exact search performance.
- The current `find` and `smart` implementation benefits a lot from metadata-first filtering before reading/parsing files.
- The interesting tradeoff is whether one `find` or `smart` query saves several follow-up `grep` + `read` steps.

For a representative `jcode` query, rough human-readable output sizes were:

- `grep`: 23 lines / 687 bytes
- `find`: 81 lines / 2754 bytes
- `smart`: 191 lines / 6961 bytes

See [docs/BENCHMARKS.md](docs/BENCHMARKS.md) for commands and reproduction details.

## Limitations

Current limitations are intentional:

- `grep` is still slower than `rg`
- `smart` uses a small DSL rather than full natural language
- `smart` region expansion is still conservative
- there is no persistent index yet
- ranking is lexical/structural, not embedding-based

## Scripts

- `scripts/smoke.sh <repo-path>` — quick manual smoke test
- `scripts/benchmark.sh <repo-path>` — reproducible benchmark run

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- [docs/INTERFACE.md](docs/INTERFACE.md)
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
