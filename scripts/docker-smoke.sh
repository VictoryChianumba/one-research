#!/usr/bin/env bash
# Smoke-test a built One-Research image. Run by CI after `docker build`;
# also works locally:  docker build -t one-research . && scripts/docker-smoke.sh
#
# Checks the properties the image promises, not just that it builds:
#   1. runs as the non-root `app` user (uid 10001)
#   2. no API keys baked into the image config or layer history
#   3. the TUI starts and draws when given a TTY (`-t`)
#   4. state written to the mounted volume survives into a new container
#   5. a key passed with `-e` is not written into that volume
#
# The app runs with --network none so the check is hermetic: the UI must
# come up even when every feed fetch fails.
set -euo pipefail

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
trap cleanup EXIT

pass() { echo "ok   $*"; }
fail() {
  echo "FAIL $*" >&2
  if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    local msg="${*//'%'/'%25'}"; msg="${msg//$'\n'/'%0A'}"
    echo "::error title=docker smoke test::${msg}"
  fi
  failures=$((failures + 1))
}

# Run the TUI detached with a pseudo-TTY, wait, then stop it.
run_tui() {
  docker run -d -t --network none --name "$name" \
    -e ONE_RESEARCH_CORE_API_KEY="$secret" \
    -v "$vol:$state" "$img" >/dev/null
  sleep 8
}
stop_tui() {
  docker stop -t 3 "$name" >/dev/null
  docker rm "$name" >/dev/null
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
run_tui
if [[ "$(docker inspect -f '{{.State.Running}}' "$name")" == "true" ]]; then
  pass "TUI still running after 8s"
else
  fail "container exited early (exit code $(docker inspect -f '{{.State.ExitCode}}' "$name")):"$'\n'"$(docker logs "$name" 2>&1 | tail -c 1500)"
fi
# Entering the alternate screen (ESC [ ? 1049 h) means the TUI took over
# the terminal and is drawing.
if docker logs "$name" 2>&1 | grep -q $'\e\[?1049h'; then
  pass "TUI entered the alternate screen"
else
  fail "no alternate-screen escape in output; the TUI never drew"
fi
stop_tui

# 4. State persists across containers.
first="$(in_volume ls -A "$state")"
if [[ -n "$first" ]]; then
  pass "state written to the volume: $(echo "$first" | tr '\n' ' ')"
else
  fail "nothing written to $state"
fi
run_tui
stop_tui
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
