# agentgrep Benchmarks

## Purpose

Define how agentgrep should be evaluated.

Claims about speed, quality, or token efficiency should be reproducible.

## Evaluation axes

1. **Latency**
2. **Resource usage**
3. **Result quality**
4. **Context efficiency**

## Baselines

Compare against overlapping capabilities in:

- `rg`
- GNU `grep`
- `fff` for file discovery
- ColGrep for heavier concept retrieval
- manual `rg + read` workflows for agent investigation

## Measurement categories

### Exact search
Measure:
- literal identifier query latency
- regex latency
- result completeness
- ordering stability

### File finding
Measure:
- exact-ish basename queries
- partial path queries
- approximate path/name queries
- top-k file quality

### Smart investigation queries
Measure:
- relation query latency
- top-k usefulness
- whether one query replaces several manual tool calls
- whether output reduces follow-up reads

## Example smart queries

- `where is auth_status called from`
- `where is auth_status rendered`
- `where is remote_available_models populated`
- `what handles scroll events`
- `where does provider_name come from`
- `how is lsp implemented`

## Metrics

### Latency
- wall-clock
- cold vs warm path
- p50 / p95 where meaningful

### Resource usage
- peak RSS
- cache/index size
- build/index time if any

### Quality
- top-1 success
- top-3 success
- top-10 success
- correct region kind near the top
- implementation vs incidental mention correctness

### Context efficiency
- number of files shown
- number of regions shown
- average output size
- whether the result packet avoided a follow-up `read`

## Benchmark philosophy

The important comparison is not just:
- is agentgrep faster than X?

It is also:
- did agentgrep reduce the total investigation cost?

So benchmark design should reflect both runtime cost and downstream token/read cost.
