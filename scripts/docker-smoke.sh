#!/usr/bin/env bash
# Smoke-test a built One-Research image. Run by CI after `docker build`;
# also works locally:  docker build -t one-research . && scripts/docker-smoke.sh
#
# Checks the properties the image promises, not just that it builds:
#   1. runs as the non-root `app` user (uid 10001)
#   2. no API keys baked into the image config or layer history
#   3. the TUI starts and keeps running under `docker run -it`
#   4. state written to the mounted volume survives into a new container
#   5. a key passed with `-e` is not written into that volume
#
# The app runs with --network none so the check is hermetic: the UI must
# come up even when every feed fetch fails.
#
# The TUI is driven the way a user runs it — `docker run -it` from a real
# terminal — via `script`, with the pseudo-terminal sized 40x120. A
# detached `docker run -d -t` would give the app a 0x0 terminal, which it
# does not survive (ratatui index panic); that is an app bug, not an image
# one, and not what users hit.
set -euo pipefail

# Needs GNU `timeout` (macOS: `brew install coreutils`).
command -v timeout >/dev/null || { echo "timeout(1) not found; on macOS: brew install coreutils" >&2; exit 2; }

img="${1:-one-research}"
vol="one-research-smoke-$$"
name="one-research-smoke-$$"
secret="smoke-test-secret-$$-do-not-persist"
state=/home/app/.config/one-research
failures=0

cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker volume rm -f "$vol" >/dev/null 2>&1 || true
}
pass() { echo "ok   $*"; }
fail() {
  echo "FAIL $*" >&2
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    local msg="${*//'%'/'%25'}"; msg="${msg//$'\n'/'%0A'}"
    echo "::error title=docker smoke test::${msg}"
  fi
  failures=$((failures + 1))
}

screen="$(mktemp)"
trap 'cleanup; rm -f "$screen"' EXIT

# Run `sh -c CMD` inside a fresh pseudo-terminal, recording what it draws.
with_pty() {
  if script --version 2>/dev/null | grep -q util-linux; then
    script -qec "$1" "$screen" >/dev/null 2>&1
  else # BSD/macOS script
    script -q "$screen" sh -c "$1" >/dev/null 2>&1
  fi
}

# Run the TUI for 8 seconds in a 40x120 terminal, as `docker run -it`.
# Prints docker's exit status: 124 means it was still running when the
# timeout stopped it (the pass case); anything else means it exited.
run_tui() {
  local rc=0
  with_pty "stty rows 40 cols 120; exec timeout 8 docker run --rm -it \
    --network none --name $name -e ONE_RESEARCH_CORE_API_KEY=$secret \
    -v $vol:$state $img" || rc=$?
  # timeout stops the client; make sure the container is gone too.
  docker rm -f "$name" >/dev/null 2>&1 || true
  echo "$rc"
}
# Run a command in a throwaway container sharing the state volume.
in_volume() {
  docker run --rm --network none -v "$vol:$state" --entrypoint "$1" "$img" "${@:2}"
}

# 1. Non-root.
uid="$(docker run --rm --entrypoint id "$img" -u)"
[[ "$uid" == "10001" ]] && pass "runs as uid 10001" || fail "runs as uid $uid, expected 10001"

# 2. No secrets in the image.
if docker image inspect "$img" --format '{{json .Config.Env}}' | grep -q 'ONE_RESEARCH_'; then
  fail "image config sets ONE_RESEARCH_* environment variables"
else
  pass "no ONE_RESEARCH_* variables in image config"
fi
if docker history --no-trunc "$img" | grep -Eiq 'ONE_RESEARCH_|api_key'; then
  fail "image layer history mentions API keys"
else
  pass "no API keys in layer history"
fi

# 3. The TUI starts and draws.
docker volume create "$vol" >/dev/null
rc="$(run_tui)"
if [[ "$rc" == "124" ]]; then
  pass "TUI still running after 8s"
else
  fail "TUI exited early (status $rc). Terminal:"$'\n'"$(tail -c 800 "$screen" | tr -d '\033')"$'\n'"App log:"$'\n'"$(in_volume tail -n 15 "$state/one-research.log" 2>&1 || true)"
fi
# Entering the alternate screen (ESC [ ? 1049 h) means the TUI took over
# the terminal and is drawing.
if grep -q $'\e\[?1049h' "$screen"; then
  pass "TUI entered the alternate screen"
else
  fail "no alternate-screen escape in output; the TUI never drew"
fi

# 4. State persists across containers.
first="$(in_volume ls -A "$state")"
if [[ -n "$first" ]]; then
  pass "state written to the volume: $(echo "$first" | tr '\n' ' ')"
else
  fail "nothing written to $state"
fi
rc="$(run_tui)"
[[ "$rc" == "124" ]] || fail "second run exited early (status $rc)"
second="$(in_volume ls -A "$state")"
missing="$(comm -23 <(echo "$first" | sort) <(echo "$second" | sort))"
[[ -z "$missing" ]] && pass "state from the first run survived a new container" \
  || fail "files lost between runs: $missing"

# 5. The -e key is not persisted.
if in_volume grep -rqF "$secret" "$state"; then
  fail "API key passed with -e was written into the state volume"
else
  pass "API key passed with -e is not in the state volume"
fi

if (( failures > 0 )); then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "all docker smoke checks passed"
