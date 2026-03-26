# agentgrep Benchmarks

This document records a real benchmark snapshot for the current implementation and shows how to reproduce it.

## Goal

Benchmark `agentgrep` on the things it is actually trying to do:

1. exact search latency
2. ranked file discovery latency
3. structured investigation latency
4. downstream usefulness relative to manual search workflows

`agentgrep` is **not** trying to beat `rg` on raw exact-search speed.
For exact search, `rg` is the baseline to respect.
The real question is whether `find` and `smart` save enough follow-up searching and reading to justify their extra work.

## Environment

Benchmark snapshot captured on:

- OS: Linux 6.18
- CPU: Intel Core Ultra 7 256V
- Binary: `cargo build --release`
- Benchmark tool: `hyperfine`
- Search target repo: `jcode`
- Indexing: none
- Daemon: none

## Commands used

### Exact literal search

```bash
./target/release/agentgrep grep --path /home/jeremy/jcode transcription > /dev/null
rg -n transcription /home/jeremy/jcode > /dev/null
```

### Exact regex search

```bash
./target/release/agentgrep grep --regex --path /home/jeremy/jcode 'transcript|voice|dictation|speech' > /dev/null
rg -n -e 'transcript|voice|dictation|speech' /home/jeremy/jcode > /dev/null
```

### Ranked file discovery

```bash
./target/release/agentgrep find --path /home/jeremy/jcode transcription transcript voice dictate speech input message > /dev/null
```

### Structured investigation

```bash
./target/release/agentgrep smart --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui > /dev/null
```

## Results

### Raw latency

| Case | Mean ± σ |
| --- | ---: |
| `agentgrep grep --path /home/jeremy/jcode transcription` | **32.0 ms ± 4.0 ms** |
| `rg -n transcription /home/jeremy/jcode` | **6.3 ms ± 1.9 ms** |
| `agentgrep grep --regex --path /home/jeremy/jcode 'transcript\|voice\|dictation\|speech'` | **36.7 ms ± 12.8 ms** |
| `rg -n -e 'transcript\|voice\|dictation\|speech' /home/jeremy/jcode` | **5.2 ms ± 1.0 ms** |
| `agentgrep find --path /home/jeremy/jcode transcription transcript voice dictate speech input message` | **174.7 ms ± 25.4 ms** |
| `agentgrep smart --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui` | **208.4 ms ± 22.9 ms** |

### Relative speed notes

- `rg` was about **5.1× faster** than `agentgrep grep` for the literal case.
- `rg` was about **7.1× faster** than `agentgrep grep --regex` for the regex case.
- `find` and `smart` both landed around **~175–210 ms** on the `jcode` repo with no index.

## Representative output sizes

Measured with `wc` on human-readable output:

| Mode | Lines | Words | Bytes |
| --- | ---: | ---: | ---: |
| `grep` | 23 | 66 | 687 |
| `find` | 81 | 367 | 2754 |
| `smart` | 191 | 707 | 6961 |

This is a useful reminder that latency is only part of the story: `smart` is returning meaningfully more structured context than a plain exact match search.

## Interpretation

### `grep`

`agentgrep grep` is currently slower than `rg`, which is expected.

That does **not** mean it is failing. It means:

- `rg` remains the reference point for pure exact search
- `agentgrep grep` still needs optimization if raw exact-search speed becomes a priority
- the richer grouped output is the main tradeoff today

### `find`

`find` is doing more than matching text:

- walking files
- extracting structure
- assigning roles
- scoring candidate files
- emitting compact outlines

~200 ms for an unindexed, one-shot ranked file discovery pass is a reasonable starting point.

### `smart`

`smart` is currently in the same rough latency band as `find`, while also:

- parsing a structured DSL
- biasing toward relation-aware evidence
- ranking regions within files
- optionally inlining small full regions

That is the mode where the main payoff is expected: fewer follow-up `grep` + `read` calls.

## Quality / workflow questions that still matter

Latency alone is not enough.
A better future benchmark suite should also measure:

- top-1 correctness
- top-3 correctness
- whether the best file is present
- whether the best region is present
- whether the result avoided a follow-up `read`
- whether one `smart` query replaced several manual search steps

## Reproducing the benchmark

From the repo root:

```bash
cargo build --release
scripts/benchmark.sh /home/jeremy/jcode
```

## Next optimizations worth exploring

1. speed up `grep` file scanning and matching
2. avoid repeated full-structure extraction where possible
3. improve `smart` region selection so more queries end in a directly usable answer packet
4. add a quality benchmark set, not just latency numbers
