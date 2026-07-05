# agentgrep wall-clock profile (baseline, 2026-07-05)

Host: XPS 13 (8 CPU, Intel Ultra 7 256V), warm page cache, but NOISY host
(load avg ~10 from other jcode agents; treat absolute numbers as +/-20%,
relative shares are stable). agentgrep b01b804, rg 15.1.0.

Method:
- hyperfine -N end-to-end (agentgrep vs identical raw rg command lines).
- /tmp/agprof harness: links agentgrep as a lib, times `run_grep`/`run_find`/
  `run_smart` via public API, then replicates each internal phase serially
  (rg subprocess -> parse -> re-read matched files -> structure extraction,
  dense-skip threshold 24 honored) and reports medians.
- strace -c -f for syscall counts; perf stat permitted (paranoid=2, :u events).

## 1. grep, sparse query (jcode, "transcription": 4 files / 6 matches)

| Phase                                   | median ms | share of run_grep |
|-----------------------------------------|-----------|-------------------|
| rg subprocess (fork/exec+scan+pipe)     | 8.5-10.4  | ~93%              |
|   of which pure fork/exec+pipe overhead | 0.4-0.8   | ~5-8%             |
| parse rg plain output                   | 0.005     | ~0%               |
| re-read 4 matched files (167 KB)        | 0.06-0.09 | ~1%               |
| structure extraction + grouping         | 0.42-0.49 | ~4-5%             |
| render                                  | 0.015     | ~0%               |
| run_grep total (lib call)               | 9.1-11.2  |                   |
| agentgrep CLI startup (--version)       | 0.57      |                   |

hyperfine end-to-end (50 runs): agentgrep grep 12.6ms vs raw rg 10.3ms
(delta 2.3ms). paths-only 11.6ms vs rg -l --null 10.1ms (delta 1.5ms).
paths-only delta is almost pure fork/exec + CLI startup + path sort.

## 2. grep, dense query (jcode, "render": 464 files / 6553 matches)

| Phase (serial replication)              | median ms | notes                          |
|-----------------------------------------|-----------|--------------------------------|
| rg subprocess                           | 11.7-15.0 |                                |
| parse rg plain output                   | 1.5       | string split + BTreeMap        |
| re-read 464 matched files (12.9 MB)     | 5.0-5.2   | serial; parallel inside run_grep |
| structure extraction (dense-skip aware) | 27.1      | serial; /8 threads inside run_grep |
| render                                  | 1.8-2.1   |                                |
| run_grep total (parallel, lib)          | 25-29     |                                |

hyperfine end-to-end: agentgrep 34.4ms vs raw rg 9.1ms => 25ms overhead,
dominated by structure extraction (even parallelized) + re-read + parse.
perf stat: 68.6ms task-clock over 30ms elapsed (parallelism ~2.3x),
1608 page-faults.

## 3. grep on /usr/src/linux ("kmalloc_array": 10 files / 47 matches, 11.2 MB matched files)

| Phase                     | median ms |
|---------------------------|-----------|
| rg subprocess             | 29.6      |
| parse                     | 0.014     |
| re-read matched (11.2 MB) | 3.3       |
| structure extraction      | 11.2      |
| run_grep total            | 45.5      |

hyperfine: agentgrep 51.2ms vs rg 32.7ms (delta 18.5ms, mostly structure
extraction of large C files + re-read). paths-only 30.7ms ~= rg -l 32.9ms
(delta ~0: paths-only adds nothing measurable at this scale).

## 4. grep on synthetic corpus (/tmp/agentgrep-synthetic-bench, "transcription": 2 files / 5 matches)

run_grep 8.0ms, rg subprocess 7.7ms (96%), everything else <0.05ms.
hyperfine: agentgrep 9.0ms vs rg 8.0ms.

## 5. find

find does NOT shell to rg: it walks (ignore crate WalkBuilder) then
reads+extracts structure only for path-evidence candidates.

| Corpus / query           | run_find | walk  | candidate read+structure | render |
|--------------------------|----------|-------|--------------------------|--------|
| jcode "search" (1489 files, 6 cand) | 9.6ms | 8.9ms | 0.74ms | 0.02ms |
| linux "kmalloc" (19323 files, 0 cand) | 52ms | 63ms* | 1.0ms | 0ms |
| synth "transcript" (3805 files, 2 cand) | 7.4ms | 6.4ms | 0.26ms | 0.01ms |

*noise; walk and run_find are the same work. find is ~90-100% directory walk.
Note walk (serial collect) costs ~9ms on jcode where parallel rg walks+scans
the same tree in ~8ms total.

## 6. trace

trace reads EVERY file in scope serially (run_smart loop: read_text_file +
to_ascii_lowercase + structure for subject-matching files).

| Corpus / query | run_smart | walk | serial read ALL files | lowercase extra | render |
|----------------|-----------|------|----------------------|-----------------|--------|
| jcode subject:transcription relation:rendered | 81-84ms | 8.4ms | 66-68ms (25.3 MB, 1489 files) | ~2ms | 0.05ms |
| linux subject:kmalloc_array relation:defined | 508ms | 107ms | 220ms (76.7 MB, 19323 files) | - | 0.03ms |
| synth subject:transcription relation:defined | 19.6ms | 7.8ms | 10.4ms | - | 0.01ms |

hyperfine end-to-end: jcode trace 66.7ms (User 10.9ms, System 54.7ms:
syscall-bound serial file reads), synth trace 18.4ms.

## Syscall counts (strace -c -f, jcode)

- grep transcription: 14 execve (1 rg + shell wrappers under strace), 9 clone3,
  1817 openat, 1810 statx, 4538 read. rg alone: 1802 openat, 1800 statx,
  4493 read. => agentgrep adds only ~15 opens (matched files + dyld) in
  sparse case; rg's own walk/scan dominates syscalls.
- find search: 1 execve, 328 openat, 3364 statx (walk stats everything).
- trace: 1 execve, 1811 openat (reads every file), 4847 statx.

## Where the real savings are (implementer guidance)

1. Sparse grep (the advertised 10.4 vs 8.7ms case): gap is ~1.5-2.3ms and is
   almost entirely rg fork/exec+pipe (0.4-0.8ms) + CLI/alloc startup + output
   piping. Structure/re-read are <0.5ms. In-process search (grep-searcher/
   ignore crates) removes fork/exec but must match rg's parallel walk speed
   to win; ceiling of saving here is ~2ms.
2. Dense grep: structure extraction is the whale (27ms serial, ~26ms of the
   25ms end-to-end gap together with re-read 5ms + parse 1.5ms + render 2ms).
   Caching structure per file, lowering the dense threshold, or lazy
   extraction (only for rendered groups) are real, large wins.
3. Matched-file re-read: 0.06ms (sparse) to 5ms (dense jcode) / 3.3ms (linux).
   Real but secondary; folding into an in-process searcher gets it for free.
4. Render: <= 2ms even at 6553 matches. Not worth optimizing.
5. find: 90-100% walk cost. Parallel walk (ignore::WalkParallel) is the only
   lever; on linux tree that is 50-60ms serial.
6. trace: worst offender. Serial whole-corpus read (66ms on jcode, 220ms on
   linux) + walk. Should pre-filter with rg/in-process matcher like grep does,
   or parallelize; 5-8x wins available.

Raw outputs: /tmp/agprof/results/, harness source: /tmp/agprof/src/main.rs.
