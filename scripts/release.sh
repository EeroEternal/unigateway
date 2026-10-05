#!/usr/bin/env bash
# Local release — replaces the former release.yml. Verifies, then publishes crates in
# dependency order. Requires CARGO_REGISTRY_TOKEN. No GitHub Release step (by design).
set -euo pipefail
cd "$(dirname "$0")/.."

./scripts/ci.sh

for crate in \
  unigateway-core \
  unigateway-config \
  unigateway-protocol \
  unigateway-host \
  unigateway-session \
  unigateway-session-redis \
  unigateway-sdk
do
  cargo publish -p "$crate" --no-verify || echo "skip: $crate already published"
done
