#!/usr/bin/env bash
# Local CI — replaces the former GitHub Actions workflows. Run before every commit / release.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== fmt"
cargo fmt --all -- --check

echo "== clippy"
cargo clippy --workspace --all-targets -- -D warnings

echo "== build"
cargo build --workspace

echo "== test"
cargo test --workspace

echo "== unigateway-sdk feature sets"
for feature in core protocol host testing; do
  cargo check -p unigateway-sdk --no-default-features --features "$feature"
  cargo test -p unigateway-sdk --no-default-features --features "$feature"
done

echo "== unigateway-host testing feature"
cargo test -p unigateway-host --features testing

echo "CI OK"
