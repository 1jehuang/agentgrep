# agentgrep Smart Output

## Purpose

This document defines the default return shape for agentgrep's three core modes, with special focus on `smart`.

The central principle is:

> return the smallest useful context packet that helps the agent stop searching and move to understanding or editing.

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

## 1. `grep`
Default unit:

> file -> exact matches

Recommended default text shape:

```text
query: auth_status
matches: 6 in 3 files

src/auth/mod.rs
  matches:
    - auth_status @ 218
      kind: definition
      pub fn auth_status() -> AuthStatus
    - auth_status @ 241
      kind: reference
      let status = auth_status();

src/tui/app.rs
  matches:
    - render_status_bar @ 9005
      kind: callsite
      let status = auth_status();
```

Important rule:
- match set must remain exact and exhaustive
- structure must remain lightweight enough not to distort grep semantics

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

## 3. `smart`
Default unit:

> ranked file -> compact structure -> ranked relevant regions

Recommended default text shape:

```text
query: where is auth_status rendered
interpreted as:
  subject: auth_status
  relation: rendered

top results: 3 files, 4 regions
best answer likely in src/tui/app.rs

1. src/tui/app.rs
   role: ui/app
   structure:
     - impl App
     - fn render_status_bar
     - fn draw_header
     ... 12 more symbols
   regions:
     - render_status_bar @ 9005-9017 (13 lines)
       kind: render-site call
       snippet:
         let status = auth_status();
       why:
         - exact subject match
         - inside render-like function
         - ui/app file

2. src/auth/mod.rs
   role: auth/core
   structure:
     - pub fn auth_status
     ... 5 more symbols
   regions:
     - auth_status @ 218-246 (29 lines)
       kind: definition
       snippet:
         pub fn auth_status() -> AuthStatus
       why:
         - likely definition of subject
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
