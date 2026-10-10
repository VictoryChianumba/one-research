#!/usr/bin/env bash
# Run the same checks as .github/workflows/ci.yml, locally.
#
# Uses the toolchain pinned in rust-toolchain.toml. Checks that need an
# extra tool say so and are skipped explicitly rather than silently.
set -Eeuo pipefail
cd "$(dirname "$0")"

step() { printf '\n==> %s\n' "$*"; }

step "rustfmt"
cargo fmt --all --check

step "clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings

step "tests (throwaway \$HOME)"
scripts/test.sh

step "architecture tripwires"
for script in scripts/check-*.sh; do
  bash "$script"
done

step "cargo audit"
if command -v cargo-audit >/dev/null; then
  cargo audit
else
  echo "SKIPPED: cargo-audit not installed (cargo install --locked cargo-audit)"
fi

step "MSRV 1.88.0"
if rustup toolchain list | grep -q '^1\.88\.0'; then
  cargo +1.88.0 check --workspace --all-targets --locked
else
  echo "SKIPPED: toolchain 1.88.0 not installed (rustup toolchain install 1.88.0)"
fi
