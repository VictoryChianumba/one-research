#!/usr/bin/env bash

# Deps
# cargo install --locked cargo-audit cargo-edit cargo-udeps cargo-geiger cargo-crev cargo-deny

set -Eeuo pipefail

ci () {
  cargo update --verbose
  cargo upgrade --verbose
  cargo audit

  # Architectural tripwires. Cheap grep checks that catch regressions
  # before the test suite would.
  #   - check-render-purification.sh: ADR-001/2/3 (per-pane composition root)
  #   - check-ingestion-seam.sh:      ADR-004 (Source / EnrichmentSource seam)
  scripts/check-render-purification.sh
  scripts/check-ingestion-seam.sh

  cargo +nightly check && cargo +nightly fix --allow-dirty && cargo +nightly clippy --fix --allow-dirty && cargo +nightly fmt --all && cargo +nightly test
  #cargo +nightly fmt --all
  #cargo +nightly clippy --all-targets --all-features -- -Dwarnings
  #cargo test

  cargo +nightly udeps --all-targets
  # cargo udeps --all-targets
}

ci
