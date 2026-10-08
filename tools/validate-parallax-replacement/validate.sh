#!/usr/bin/env bash
set -euo pipefail
if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "usage: $0 REPOSITORY BEVY_RENDER_0_18_1_DIR [OUTPUT_DIR]" >&2
  exit 2
fi
repo=$(cd "$1" && pwd)
render=$(cd "$2" && pwd)
root=$(cd "$(dirname "$0")" && pwd)
output=${3:-"$root"}
mkdir -p "$output"
output=$(cd "$output" && pwd)
cd "$root"
{
  rustc --version
  cargo --version
  cargo fmt --all -- --check
  cargo clippy --locked --offline --all-targets -- -D warnings
  cargo run --locked --offline -- "$repo" "$render" "$output/validation.json"
} 2>&1 | tee "$output/validation.log"
