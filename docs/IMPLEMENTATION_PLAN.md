# agentgrep Implementation Plan

## Purpose

This document turns the architecture into buildable milestones.

## Guiding rule

Each milestone should be:

- locally scoped
- testable
- measurable
- incrementally useful

Avoid speculative infrastructure until it clearly earns its keep.

## Milestone 0 — scaffold

### Deliverables
- Rust crate initialized
- CLI skeleton for `grep`, `find`, `trace`
- command parsing tests
- benchmark harness skeleton

### Exit criteria
- `agentgrep --help` works
- `agentgrep grep/find/trace --help` works
- debug and release builds succeed

## Milestone 1 — exact lexical core

### Deliverables
- repository walking
- include/exclude/type filtering
- literal grep
- regex grep
- deterministic text output
- JSON output for grep

### Exit criteria
- `agentgrep grep foo` is trustworthy
- exactness semantics are locked

## Milestone 2 — path finder

### Deliverables
- path/basename candidate generation
- identifier-aware normalization
- ranked `find` output
- debug-score support for `find`

### Exit criteria
- `find` is meaningfully better than file-list grep for common navigation tasks

## Milestone 3 — planner and query interpretation

### Deliverables
- shared query normalization
- intent classification
- subject extraction
- relation phrase detection
- debug-plan output

### Exit criteria
- planner decisions are stable and understandable on curated example queries

## Milestone 4 — trace mode v1

### Deliverables
- internal multi-pass lexical probing
- file/content candidate fusion
- grouped output packet
- relation-aware ranking profiles
- region and structure extraction sufficient for useful output

### Exit criteria
- `trace` often replaces several manual search/read steps on representative tasks

## Milestone 5 — adaptive output shaping

### Deliverables
- compact structure summaries
- region-kind labels
- nearby match merging
- adaptive full-region inclusion for short, high-confidence regions

### Exit criteria
- output is measurably more token-efficient than naive grep+read workflows

## Milestone 6 — Tier 1 manifest cache

### Deliverables
- manifest cache build/load/clear/status
- cheap validation
- atomic writes
- no-correctness-dependency fallback path

### Exit criteria
- warm `find` improves materially
- cache absence does not break usability

## Milestone 7 — evaluation and hardening

### Deliverables
- reproducible benchmark harness
- quality task suite
- profiling notes
- UX iteration based on measurements

### Exit criteria
- we know where the architecture wins and where it still falls short

## Explicitly deferred

### Semantic sidecar
Only after lexical-first evidence says it is needed.

### Daemon/watcher mode
Not part of the default path.

### Large surface-area feature growth
Delay `outline`, `symbols`, and other top-level commands until the 3 core commands are excellent.
