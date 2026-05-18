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
  #   - check-render-purification.sh: ADR-001/2/3/5 (per-pane composition root)
  #   - check-ingestion-seam.sh:      ADR-004 (Source / EnrichmentSource seam)
  #   - check-store-seam.sh:          ADR-006 (load_json / save_json envelope)
  #   - check-item-store.sh:          ADR-007 (ItemStore encapsulates Workspace triple)
  #   - check-frame-layout.sh:        ADR-008 (FrameLayout / apply_frame_layout)
  scripts/check-render-purification.sh
  scripts/check-ingestion-seam.sh
  scripts/check-store-seam.sh
  scripts/check-item-store.sh
  scripts/check-frame-layout.sh

  cargo +nightly check && cargo +nightly fix --allow-dirty && cargo +nightly clippy --fix --allow-dirty && cargo +nightly fmt --all && cargo +nightly test
  #cargo +nightly fmt --all
  #cargo +nightly clippy --all-targets --all-features -- -Dwarnings
  #cargo test

  cargo +nightly udeps --all-targets
  # cargo udeps --all-targets
}

ci
