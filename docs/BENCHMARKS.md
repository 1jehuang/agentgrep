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
./target/release/agentgrep trace --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui > /dev/null
```

## Results

### Raw latency

| Case | Mean ± σ |
| --- | ---: |
| `agentgrep grep --path /home/jeremy/jcode transcription` | **30.9 ms ± 1.7 ms** |
| `rg -n transcription /home/jeremy/jcode` | **8.2 ms ± 0.9 ms** |
| `agentgrep grep --regex --path /home/jeremy/jcode 'transcript\|voice\|dictation\|speech'` | **44.2 ms ± 14.0 ms** |
| `rg -n -e 'transcript\|voice\|dictation\|speech' /home/jeremy/jcode` | **8.7 ms ± 1.0 ms** |
| `agentgrep find --path /home/jeremy/jcode transcription transcript voice dictate speech input message` | **6.1 ms ± 2.2 ms** |
| `agentgrep trace --path /home/jeremy/jcode subject:TranscriptMode relation:implementation kind:code path:src/tui` | **17.3 ms ± 0.6 ms** |

### Relative speed notes

- `rg` was about **3.8× faster** than `agentgrep grep` for the literal case.
- `rg` remains much faster than `agentgrep grep --regex`, though the regex run was noticeably noisier in this snapshot.
- `find` dropped to roughly **single-digit milliseconds** for this topic query because it can now reject most files from path metadata alone.
- `trace` dropped to roughly **~17 ms** for this query after adding a cheap subject/path prefilter before structure extraction and reusing pre-lowercased file lines during region scoring.

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
- the latest macro win came from reusing the shared workspace scope and parallelizing per-file grep processing
- the richer grouped output is the main tradeoff today

### `find`

`find` is doing more than matching text:

- walking files
- extracting structure
- assigning roles
- scoring candidate files
- emitting compact outlines

The current implementation is much faster than the earlier baseline for path-heavy topic queries because it avoids reading/parsing files until they survive cheap path-based filtering.

### `smart`

`trace` is still doing more work than `grep`, but after the latest filtering changes it is much cheaper for subtree-constrained queries while still:

- parsing a structured DSL
- biasing toward relation-aware evidence
- ranking regions within files
- optionally inlining small full regions

That is the mode where the main payoff is expected: fewer follow-up `grep` + `read` calls.

The latest improvement came from two simple macro optimizations:

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
- whether one `smart` query replaced several manual search steps

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
