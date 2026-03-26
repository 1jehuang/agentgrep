#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <repo-path>" >&2
  exit 1
fi

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "error: hyperfine is required" >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
  echo "error: rg is required for baseline comparisons" >&2
  exit 1
fi

REPO_PATH="$1"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT_DIR/target/release/agentgrep"

if [[ ! -x "$BIN" ]]; then
  echo "== building release binary =="
  cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release
fi

echo "== agentgrep benchmark on: $REPO_PATH =="
echo
uname -a
echo
if command -v lscpu >/dev/null 2>&1; then
  lscpu | sed -n '1,20p'
  echo
fi

echo "-- literal grep vs rg --"
hyperfine --warmup 2 --runs 10 \
  "$BIN grep --path $REPO_PATH transcription > /dev/null" \
  "rg -n transcription $REPO_PATH > /dev/null"

echo
echo "-- regex grep vs rg --"
hyperfine --warmup 2 --runs 10 \
  "$BIN grep --regex --path $REPO_PATH 'transcript|voice|dictation|speech' > /dev/null" \
  "rg -n -e 'transcript|voice|dictation|speech' $REPO_PATH > /dev/null"

echo
echo "-- find latency --"
hyperfine --warmup 2 --runs 10 \
  "$BIN find --path $REPO_PATH transcription transcript voice dictate speech input message > /dev/null"

echo
echo "-- smart latency --"
hyperfine --warmup 2 --runs 10 \
  "$BIN smart --path $REPO_PATH subject:TranscriptMode relation:implementation kind:code path:src/tui > /dev/null"

echo
echo "-- representative output sizes --"
echo "grep:"
$BIN grep --path "$REPO_PATH" transcription | wc
echo "find:"
$BIN find --path "$REPO_PATH" transcription transcript voice dictate speech input message | wc
echo "smart:"
$BIN smart --path "$REPO_PATH" subject:TranscriptMode relation:implementation kind:code path:src/tui | wc
