#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT_DIR/target/release/agentgrep"
SYNTH_PATH="${AGENTGREP_SYNTH_PATH:-/tmp/agentgrep-synthetic-bench}"
JCODE_PATH="${AGENTGREP_JCODE_PATH:-/home/jeremy/jcode}"
LINUX_PATH="${AGENTGREP_LINUX_PATH:-/usr/src/linux}"
RUNS="${AGENTGREP_BENCH_RUNS:-8}"
WARMUP="${AGENTGREP_BENCH_WARMUP:-2}"

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "error: hyperfine is required" >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
  echo "error: rg is required" >&2
  exit 1
fi

if [[ ! -x "$BIN" ]]; then
  echo "== building release binary =="
  cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release
fi

python3 "$ROOT_DIR/scripts/generate_synthetic_corpus.py" "$SYNTH_PATH" >/dev/null

benchmark_pair() {
  local label="$1"
  local cmd_a="$2"
  local cmd_b="$3"
  echo "-- $label --"
  hyperfine --warmup "$WARMUP" --runs "$RUNS" "$cmd_a" "$cmd_b"
  echo
}

benchmark_single() {
  local label="$1"
  local cmd="$2"
  echo "-- $label --"
  hyperfine --warmup "$WARMUP" --runs "$RUNS" "$cmd"
  echo
}

run_suite() {
  local name="$1"
  local repo="$2"
  local literal="$3"
  local regex="$4"
  local find_terms="$5"
  local trace_terms="$6"

  if [[ ! -d "$repo" ]]; then
    echo "== skipping $name: missing path $repo =="
    echo
    return 0
  fi

  echo "== $name benchmark on: $repo =="
  echo

  benchmark_pair \
    "$name literal grep vs rg" \
    "$BIN grep --path $repo '$literal' > /dev/null" \
    "rg -n -F '$literal' $repo > /dev/null"

  benchmark_pair \
    "$name literal paths-only grep vs rg" \
    "$BIN grep --paths-only --path $repo '$literal' > /dev/null" \
    "rg -l -F '$literal' $repo > /dev/null"

  benchmark_pair \
    "$name regex grep vs rg" \
    "$BIN grep --regex --path $repo '$regex' > /dev/null" \
    "rg -n -e '$regex' $repo > /dev/null"

  benchmark_pair \
    "$name regex paths-only grep vs rg" \
    "$BIN grep --regex --paths-only --path $repo '$regex' > /dev/null" \
    "rg -l -e '$regex' $repo > /dev/null"

  benchmark_single \
    "$name find latency" \
    "$BIN find --path $repo $find_terms > /dev/null"

  benchmark_single \
    "$name trace latency" \
    "$BIN trace --path $repo $trace_terms > /dev/null"
}

run_suite \
  "synthetic" \
  "$SYNTH_PATH" \
  "transcription" \
  "transcript|voice|dictation|speech" \
  "transcription transcript voice dictate speech input message" \
  "subject:TranscriptMode relation:implementation kind:code path:src"

run_suite \
  "jcode" \
  "$JCODE_PATH" \
  "transcription" \
  "transcript|voice|dictation|speech" \
  "transcription transcript voice dictate speech input message" \
  "subject:TranscriptMode relation:implementation kind:code path:src/tui"

run_suite \
  "linux" \
  "$LINUX_PATH" \
  "spin_lock" \
  "spin_lock|mutex_lock|rwlock|semaphore" \
  "spin lock mutex scheduler irq wakeup" \
  "subject:spin_lock relation:implementation kind:code path:kernel/locking"
