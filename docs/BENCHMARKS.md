# agentgrep Benchmarks

This document records a real benchmark snapshot for the current implementation and shows how to reproduce it.

## Goal

Benchmark `agentgrep` on the things it is actually trying to do:

1. exact search latency
2. ranked file discovery latency
3. structured investigation latency
4. downstream usefulness relative to manual search workflows

`agentgrep` is **not** trying to beat `rg`'s core engine at raw exact-search speed.
For exact search, `rg` is still the baseline to respect.
The real question is whether `find` and `trace` save enough follow-up searching and reading to justify their extra work.

That said, `grep` should avoid wasting work when `rg` is available locally.
The current implementation now opportunistically uses `rg` as the lexical scan backend for `grep`, then layers agentgrep's grouped structural output on top.

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
./target/release/agentgrep grep --paths-only --path /home/jeremy/jcode transcription > /dev/null
rg -n transcription /home/jeremy/jcode > /dev/null
rg -l transcription /home/jeremy/jcode > /dev/null
```

### Exact regex search

```bash
./target/release/agentgrep grep --regex --path /home/jeremy/jcode 'transcript|voice|dictation|speech' > /dev/null
./target/release/agentgrep grep --regex --paths-only --path /home/jeremy/jcode 'transcript|voice|dictation|speech' > /dev/null
rg -n -e 'transcript|voice|dictation|speech' /home/jeremy/jcode > /dev/null
rg -l -e 'transcript|voice|dictation|speech' /home/jeremy/jcode > /dev/null
```

### Ranked file discovery

```bash
./target/release/agentgrep find --path /home/jeremy/jcode transcription transcript voice dictate speech input message > /dev/null
```

### Structured investigation

```bash
./target/release/agentgrep trace --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui > /dev/null
```

## Results

### Raw latency

| Case | Mean ± σ |
| --- | ---: |
| `agentgrep grep --path /home/jeremy/jcode transcription` | **12.3 ms ± 1.4 ms** |
| `agentgrep grep --paths-only --path /home/jeremy/jcode transcription` | **5.2 ms ± 0.7 ms** |
| `rg -n transcription /home/jeremy/jcode` | **6.4 ms ± 0.8 ms** |
| `rg -l transcription /home/jeremy/jcode` | **4.8 ms ± 1.1 ms** |
| `agentgrep grep --regex --path /home/jeremy/jcode 'transcript\|voice\|dictation\|speech'` | **33.3 ms ± 4.3 ms** |
| `agentgrep grep --regex --paths-only --path /home/jeremy/jcode 'transcript\|voice\|dictation\|speech'` | **6.2 ms ± 0.9 ms** |
| `rg -n -e 'transcript\|voice\|dictation\|speech' /home/jeremy/jcode` | **6.9 ms ± 0.8 ms** |
| `rg -l -e 'transcript\|voice\|dictation\|speech' /home/jeremy/jcode` | **7.1 ms ± 0.5 ms** |
| `agentgrep find --path /home/jeremy/jcode transcription transcript voice dictate speech input message` | **6.0 ms ± 1.7 ms** |
| `agentgrep trace --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui` | **25.6 ms ± 1.1 ms** |

### Relative speed notes

- Human-readable literal `grep` is now about **1.9×** slower than `rg -n`, down from roughly **3.6-3.8×** in the earlier baseline.
- `grep --paths-only` is now within about **8%** of `rg -l` for the literal case.
- `grep --regex --paths-only` is now effectively at parity with `rg -l -e ...` for this query.
- Human-readable regex `grep` is still slower because it pays the extra cost of reopening each matched file, extracting structure, and grouping matches into symbols after the fast lexical pass.
- `find` stays in roughly **single-digit milliseconds** for this topic query because it can reject most files from path metadata alone.
- `trace` remains much heavier than `grep`; this snapshot came in around **~26 ms** for the chosen subtree-constrained query.

## Representative output sizes

Measured with `wc` on human-readable output:

| Mode | Lines | Words | Bytes |
| --- | ---: | ---: | ---: |
| `grep` | 23 | 66 | 687 |
| `find` | 81 | 367 | 2754 |
| `trace` | 191 | 707 | 6961 |

This is a useful reminder that latency is only part of the story: `trace` is returning meaningfully more structured context than a plain exact match search.

## Interpretation

### `grep`

`agentgrep grep` is still slower than plain `rg` when it emits richer grouped output, which is expected.

That does **not** mean it is failing. It means:

- `rg` remains the reference point for pure exact search
- the latest macro win came from using `rg` as an optional lexical accelerator for `grep`, then only doing agent-specific structure work on matched files
- the richer grouped output is now the main remaining tradeoff today, not the raw lexical scan itself

### `find`

`find` is doing more than matching text:

- walking files
- extracting structure
- assigning roles
- scoring candidate files
- emitting compact outlines

The current implementation is much faster than the earlier baseline for path-heavy topic queries because it avoids reading/parsing files until they survive cheap path-based filtering.

### `trace`

`trace` is still doing more work than `grep`, but after the latest filtering changes it is much cheaper for subtree-constrained queries while still:

- parsing a structured DSL
- biasing toward relation-aware evidence
- ranking regions within files
- optionally inlining small full regions

That is the mode where the main payoff is expected: fewer follow-up `grep` + `read` calls.

The biggest earlier trace improvement came from two simple macro optimizations:

- reject files before structure extraction unless the subject appears plausible in the path/text
- lowercase each file's lines once and reuse them across region scoring instead of re-lowercasing every candidate region

## Quality / workflow questions that still matter

Latency alone is not enough.
A better future benchmark suite should also measure:

- top-1 correctness
- top-3 correctness
- whether the best file is present
- whether the best region is present
- whether the result avoided a follow-up `read`
- whether one `trace` query replaced several manual search steps

## Reproducing the benchmark

From the repo root:

```bash
cargo build --release
scripts/benchmark.sh /home/jeremy/jcode
```

## Next optimizations worth exploring

1. add a quality benchmark set, not just latency numbers
2. measure whether parallel traversal/search is worthwhile for `trace` on larger repos before adding complexity
3. improve `trace` region selection so more queries end in a directly usable answer packet
4. profile memory/output-size tradeoffs on very large repos
