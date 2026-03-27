#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <repo-path>" >&2
  exit 1
fi

REPO_PATH="$1"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN=(cargo run --manifest-path "$ROOT_DIR/Cargo.toml" --)

echo "== agentgrep smoke on: $REPO_PATH =="

echo
echo "-- grep: lsp --"
"${BIN[@]}" grep --path "$REPO_PATH" lsp | sed -n '1,25p'

echo
echo "-- find: debug socket --"
"${BIN[@]}" find --path "$REPO_PATH" debug socket | sed -n '1,25p'

echo
echo "-- outline: src/tool/lsp.rs --"
"${BIN[@]}" outline --path "$REPO_PATH" src/tool/lsp.rs | sed -n '1,25p'

echo
echo "-- smart: lsp implementation --"
"${BIN[@]}" smart --path "$REPO_PATH" subject:lsp relation:implementation kind:code path:src/tool | sed -n '1,40p'

echo
echo "-- smart: debug_socket defined --"
"${BIN[@]}" smart --path "$REPO_PATH" subject:debug_socket relation:defined kind:code path:src | sed -n '1,40p'
