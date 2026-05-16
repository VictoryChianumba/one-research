#!/usr/bin/env bash

# Deps
# cargo install --locked cargo-audit cargo-edit cargo-udeps cargo-geiger cargo-crev cargo-deny

set -Eeuo pipefail

ci () {
  cargo update --verbose
  cargo upgrade --verbose
  cargo audit

  # Slice-1 render-purification tripwires (ADR-001). Cheap grep checks
  # that catch regressions before the test suite would.
  scripts/check-render-purification.sh

  cargo +nightly check && cargo +nightly fix --allow-dirty && cargo +nightly clippy --fix --allow-dirty && cargo +nightly fmt --all && cargo +nightly test
  #cargo +nightly fmt --all
  #cargo +nightly clippy --all-targets --all-features -- -Dwarnings
  #cargo test

  cargo +nightly udeps --all-targets
  # cargo udeps --all-targets
}

ci
