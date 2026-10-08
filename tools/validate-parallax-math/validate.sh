#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
{
  rustc --version
  cargo --version
  cargo fmt --all -- --check
  cargo test --locked --offline --all-targets
  cargo clippy --locked --offline --all-targets -- -D warnings
} 2>&1 | tee validation.log
