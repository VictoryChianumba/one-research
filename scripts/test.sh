#!/usr/bin/env bash
# Run the workspace test suite against a throwaway $HOME.
#
# Why: several tests construct `App::new()`, which loads and saves real
# state under the user's config dir (~/.config/one-research/, and on macOS
# ~/Library/Application Support/one-research/ for notes and chat). Running
# plain `cargo test` against your real $HOME can overwrite your feed cache
# with test fixtures. This script is what CI runs.
#
# Extra arguments go to `cargo test`, e.g. `scripts/test.sh -p one-research`.
set -euo pipefail

# Keep the toolchain and registry where they are; only state dirs move.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
# On Linux `dirs::config_dir()` prefers these over $HOME.
unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME

tmp_home="$(mktemp -d)"
trap 'rm -rf "$tmp_home"' EXIT

# Whole workspace unless a package was named (-p/--package).
scope=(--workspace)
for arg in "$@"; do
  case "$arg" in
    -p|--package|--package=*) scope=() ;;
    --) break ;;
  esac
done

HOME="$tmp_home" cargo test ${scope[@]+"${scope[@]}"} --locked "$@"
