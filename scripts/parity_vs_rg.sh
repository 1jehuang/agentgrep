#!/usr/bin/env bash
# Parity harness: agentgrep grep vs rg ground truth across the shared
# corpus/query matrix. Writes bench-results/parity.json; exits nonzero on
# any mismatch. Idempotent; runnable from repo root or anywhere.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${AGENTGREP_BIN:-$ROOT_DIR/target/release/agentgrep}"
SYNTH_PATH="${AGENTGREP_SYNTH_PATH:-/tmp/agentgrep-synthetic-bench}"

command -v rg >/dev/null 2>&1 || { echo "error: rg is required" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "error: python3 is required" >&2; exit 1; }

if [[ ! -x "$BIN" || "${AGENTGREP_REBUILD:-0}" == "1" ]]; then
  echo "== building release binary =="
  cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release
fi

echo "== generating synthetic corpus at $SYNTH_PATH =="
python3 "$ROOT_DIR/scripts/generate_synthetic_corpus.py" "$SYNTH_PATH" >/dev/null

AGENTGREP_BIN="$BIN" AGENTGREP_SYNTH_PATH="$SYNTH_PATH" \
  exec python3 "$ROOT_DIR/scripts/parity_vs_rg.py"
