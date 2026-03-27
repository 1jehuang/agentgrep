# agentgrep Output Shapes

## Purpose

This document defines the default return shape for agentgrep's four current modes, with special focus on `trace`.

The central principle is:

> return the smallest useful context packet that helps the agent stop searching and move to understanding or editing.

Secondary principle:

> when requested, return less structure and more scriptable output instead of richer explanation.

## Core output philosophy

agentgrep should optimize:

1. **search efficiency** — fewer exploratory tool calls
2. **context efficiency** — fewer follow-up read tokens
3. **decision efficiency** — clearer next action

That means results should be:

- grouped by file/document
- enriched with compact structure
- centered on relevant regions
- explainable
- adaptive in how much code/context they inline

## Default return shape by mode

All three modes should also support:

- `--json` for structured machine-readable output
- `--paths-only` for file-only output
- `--debug-score` where ranking transparency matters

## 1. `grep`
Default unit:

> file -> matched symbols -> exact lines

Recommended default text shape:

```text
query: auth_status
matches: 6 in 3 files

src/auth/mod.rs
  symbols: 4 total, 1 matched, 3 other
    - function auth_status @ 218-246
      - @ 218 pub fn auth_status() -> AuthStatus
      - @ 241 let status = auth_status();
    - other: enum AuthStatus @ 180-210; function format_status @ 247-268; impl AuthStatus @ 269-320

src/tui/app.rs
  symbols: 6 total, 1 matched, 5 other
    - function render_status_bar @ 8990-9017
      - @ 9005 let status = auth_status();
    - other: function draw_header @ 8901-8940; function draw_footer @ 8941-8989; ... 3 more
```

Important rule:
- match set must remain exact and exhaustive
- structure must remain lightweight enough not to distort grep semantics
- unmatched structure should be a compact hint, not a full second outline dump

## 2. `find`
Default unit:

> ranked file -> compact structure summary

Recommended default text shape:

```text
query: auth status
top files: 5

1. src/auth/mod.rs
   role: auth/core
   structure:
     - enum AuthStatus
     - pub fn auth_status
     ... 5 more symbols
   why:
     - path token match
     - exact identifier variant match

2. src/tui/app.rs
   role: ui/app
   structure:
     - fn render_status_bar
     - fn draw_header
     ... 12 more symbols
   why:
     - likely UI consumer of auth status
```

## 3. `outline`
Default unit:

> known file -> symbol map

Recommended default text shape:

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
  - function execute @ 76-95 (20 lines)
```

## 4. `trace`
Default unit:

> ranked file -> usable follow-up outline -> ranked relevant regions

Recommended default text shape:

```text
query parameters:
  subject: auth_status
  relation: rendered
  support: ui
  kind: code

top results: 3 files, 4 regions
best answer likely in src/tui/app.rs

1. src/tui/app.rs
   role: ui/app
   structure:
     - impl App @ 8700-9450
     - fn render_status_bar @ 9002-9017 (16 lines)
     - fn draw_header @ 9035-9056 (22 lines)
     - fn build_auth_panel @ 9058-9104 (47 lines)
     - fn render_footer @ 9108-9130 (23 lines)
     - fn auth_status @ 9132-9140 (9 lines)
     ... 6 more symbols
   regions:
     - fn render_status_bar @ 9002-9017 (16 lines)
       kind: render-site
       full region:
         fn render_status_bar(&self, ui: &mut Ui) {
             let status = auth_status();
             ui.label(status.to_string());
             if self.show_details {
                 ui.label(self.auth_message());
             }
         }
       why:
         - exact subject match
         - relation-context aligned

2. src/auth/mod.rs
   role: auth/core
   structure:
     - enum AuthStatus @ 180-210 (31 lines)
     - pub fn auth_status @ 218-246 (29 lines)
     - fn authenticate @ 248-310 (63 lines)
     ... 4 more symbols
   regions:
     - pub fn auth_status @ 218-246 (29 lines)
       kind: definition
       snippet:
         pub fn auth_status() -> AuthStatus
       why:
         - exact subject match
```

## Required fields in a result packet

### Top-level
- raw query
- interpreted query (when applicable)
- summary counts
- best file hint when confidence is high

### Per file
- path
- role hint
- compact structure summary
- omitted structure count if truncated
- why file ranked highly

### Per region
- enclosing symbol / structural unit
- line range
- line count
- region kind
- snippet or full region
- why region ranked highly
- omitted region count if needed

## Region kinds

Useful initial region kinds for code:

- definition
- callsite
- reference
- assignment
- handler
- render-site
- import/export
- test-only reference

These labels help the agent avoid having to infer role from raw lines.

## Structure budget

The structure section should not be only a teaser.
It should act like a mini follow-up map of the file.

Recommended default behavior:

- show all directly relevant structure items first
- then fill with nearby or major symbols in source order
- show roughly 5-10 structure items by default in `find`
- show roughly 8-12 structure items by default in `trace`
- include start line always when available
- include end line and line count when cheaply available
- truncate only after the outline is genuinely useful

## Adaptive region inclusion

The result shape should not always truncate aggressively.
Sometimes the cheapest result is the whole function.

### Principle

> compact when uncertain, more complete when confident, full when cheap and decisive.

### Region output modes

#### 1. Full region
Use when:
- region is short
- confidence is high
- region is self-contained
- likely enough to answer the question directly

Example thresholds to start with:
- <= 20 lines
- <= ~1000 chars
- top ranked region(s)

#### 2. Expanded excerpt
Use when:
- region is medium-sized
- the best local chunk is clear
- full region would cost too much

#### 3. Summary/snippet only
Use when:
- region is large
- confidence is lower
- there are many competing candidates

## Useful-unit rule

The right thing to inline is not always a fixed snippet size.
For code, the ideal unit is often the **smallest enclosing useful unit**, such as:

- full short function
- short method
- concise top-level initialization block
- small builder or assignment block
- small type definition

## Compression rules

To keep results token-efficient:

- group by file
- merge nearby matches in one region
- show omitted counts (`... 10 more symbols`, `... 3 more regions`)
- prefer best-region-first ordering
- prefer best-file-first ordering

## JSON model

Even if text output is compact, JSON should preserve richer structure.

Suggested top-level shape:

```json
{
  "query": {
    "raw": "where is auth_status rendered",
    "subject": "auth_status",
    "relation": "rendered"
  },
  "summary": {
    "files": 3,
    "regions": 4,
    "best_file": "src/tui/app.rs"
  },
  "files": [
    {
      "path": "src/tui/app.rs",
      "role": "ui/app",
      "why": ["exact subject match", "render-like function", "ui file"],
      "structure": {
        "items": [
          {"kind": "impl", "label": "App"},
          {"kind": "function", "label": "render_status_bar"}
        ],
        "omitted_count": 12
      },
      "regions": [
        {
          "kind": "render-site-call",
          "enclosing_symbol": "render_status_bar",
          "start_line": 9005,
          "end_line": 9017,
          "line_count": 13,
          "snippet": "let status = auth_status();",
          "full_region_included": false
        }
      ]
    }
  ]
}
```
