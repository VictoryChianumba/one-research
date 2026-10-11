# One-Research in a container.
#
#   docker build -t one-research .
#   docker run --rm -it \
#     -e ONE_RESEARCH_CORE_API_KEY \
#     -v one-research-data:/home/app/.config/one-research \
#     one-research
#
# - It's a TUI: `-it` is required (a terminal, and keyboard input).
# - API keys come only from ONE_RESEARCH_* environment variables at run
#   time; nothing secret is baked into the image, and env-supplied keys are
#   never written to config.json (see Config::persistable).
# - All state (config, cache, notes, chat history, reading progress) lives
#   under /home/app/.config/one-research; mount a volume there to keep it.
#
# Base images are pinned by multi-arch digest (amd64 + arm64); Dependabot
# keeps the pins current. The Rust version must match rust-toolchain.toml
# (CI checks this).

ARG RUST_IMAGE=rust:1.97.0-slim-bookworm@sha256:6d220bf85c74e842a79da63997af8d2e74455c0b8847d8bb3a5888572334991d
ARG RUNTIME_IMAGE=debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587

# ── Build ─────────────────────────────────────────────────────────────────
FROM ${RUST_IMAGE} AS builder

# libasound2-dev: tread's voice playback (rodio -> cpal -> ALSA).
# libssl-dev: reqwest's default TLS backend (native-tls -> OpenSSL).
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libasound2-dev libssl-dev \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

# Cache mounts keep the registry and target/ between local rebuilds; they
# are not part of any image layer. The binary is copied out of the cache
# mount so the next stage can use it.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p one-research \
 && cp target/release/one-research /usr/local/bin/one-research

# ── Runtime ───────────────────────────────────────────────────────────────
FROM ${RUNTIME_IMAGE}

# Shared libraries the binary links against (checked by `ldd` below), CA
# roots for HTTPS, and tini as PID 1: the app installs no SIGTERM handler,
# and the kernel ignores SIGTERM for a PID 1 without one, so without an
# init `docker stop` / Ctrl-C would hang until Docker's SIGKILL.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libasound2 libssl3 tini \
 && rm -rf /var/lib/apt/lists/*

# Fixed non-root uid/gid so bind-mounted state has predictable ownership.
RUN groupadd --gid 10001 app \
 && useradd --uid 10001 --gid 10001 --create-home --home-dir /home/app \
      --shell /usr/sbin/nologin app \
 && mkdir -p /home/app/.config/one-research \
 && chown -R app:app /home/app

COPY --from=builder /usr/local/bin/one-research /usr/local/bin/one-research

# Fail the build if any linked library is missing from this stage.
RUN ! ldd /usr/local/bin/one-research | grep -q 'not found'

USER 10001:10001
ENV HOME=/home/app
WORKDIR /home/app

# No VOLUME instruction on purpose: it would silently create an anonymous
# volume per run (state that looks saved but is orphaned). Mount one
# explicitly at /home/app/.config/one-research instead.

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/one-research"]
