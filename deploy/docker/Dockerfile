# syntax=docker/dockerfile:1

# ---- builder ------------------------------------------------------------
# bookworm base so the produced glibc-linked binary matches the runtime.
# Cargo.lock pulls transitive deps requiring the `edition2024` feature
# (stabilized in Rust 1.85); 1.83 fails with "feature `edition2024` is
# required". Pinned to the version verified locally (rustc --version).
FROM rust:1.93.1-bookworm AS builder
WORKDIR /build
COPY . .
# reqwest/tokio-tungstenite use rustls, so no OpenSSL dev headers are needed.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --bin standx && \
    cp target/release/standx /build/standx

# ---- runtime ------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
# bash        — the orchestrator and observed wrapper are bash scripts
# python3     — manifest, config-diff, webhook, alert tooling
# util-linux  — flock (guard/maker process locks)
# coreutils   — sha256sum, date; perl provides shasum used by the wrapper
# curl        — supervisor webhook POSTs from the observed wrapper
# git         — the run manifest requires HEAD + a clean strategy worktree
# ca-certificates — rustls native-roots for the venue TLS handshake
RUN apt-get update && apt-get install -y --no-install-recommends \
        bash python3 util-linux coreutils perl curl git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/standx
# Ships the repository (including .git; see .dockerignore) so the manifest's
# git_sha_present / strategy_source_clean checks pass at runtime.
COPY . /opt/standx
COPY --from=builder /build/standx /opt/standx/bin/standx

# git refuses to operate on a tree owned by another uid; the manifest tooling
# runs git as the container user, so mark the deployment root as trusted.
RUN git config --system --add safe.directory /opt/standx

# Build-time gate mirroring scripts/maker_run_manifest.py: refuse to ship an
# image whose committed strategy source is missing or dirty, because the A/B
# orchestrator would invalidate every arm and critical-stop at runtime.
RUN set -eu; \
    test "$(git rev-parse HEAD | tr -d '\n' | wc -c)" -eq 40 \
      || { echo 'ERROR: no committed HEAD; build from a clean release checkout' >&2; exit 1; }; \
    dirty="$(git status --porcelain --untracked-files=all -- \
        Cargo.toml Cargo.lock crates/standx-maker crates/standx-sdk \
        crates/standx-cli examples/maker.toml)"; \
    test -z "$dirty" \
      || { echo 'ERROR: strategy source is dirty; build from a clean release commit:' >&2; \
           echo "$dirty" >&2; exit 1; }

# Env-only auth: credentials come from STANDX_JWT / STANDX_PRIVATE_KEY in the
# env_file (see entrypoint.sh), never a credentials.enc file. XDG_DATA_HOME is
# still set for other state the app may write under this writable layout.
ENV XDG_DATA_HOME=/opt/standx/state
# var/lock is where STANDX_MAKER_LOCK_PATH / STANDX_STAGE2_AB_LOCK_PATH should
# point by default — container-local, not the host's /run/lock (see
# docker-compose.yml). The app also create_dir_all()s the lock's parent at
# runtime, but this keeps the image's writable layout explicit.
RUN mkdir -p /opt/standx/state/standx /opt/standx/var/standx /opt/standx/var/lock

COPY deploy/docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
