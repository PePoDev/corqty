# syntax=docker/dockerfile:1
FROM rust:1.96-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations
# target/ lives in a cache mount, so the binaries are copied out in the same RUN.
RUN --mount=type=cache,id=corqty-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=corqty-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=corqty-target,target=/workspace/target,sharing=locked \
    cargo build --release --locked -p corqty-api -p corqty-migrator \
    && install -D -m 0755 -t /out target/release/corqty-api target/release/corqty-migrator

# The runtime glibc must match the builder's (bookworm).
FROM debian:bookworm-slim AS runtime
RUN groupadd --system --gid 10001 corqty \
    && useradd --system --uid 10001 --gid corqty --no-create-home --shell /usr/sbin/nologin corqty

FROM runtime AS migrate
COPY --from=builder /out/corqty-migrator /usr/local/bin/
USER 10001:10001
ENTRYPOINT ["corqty-migrator"]

FROM runtime AS api
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /out/corqty-api /usr/local/bin/
USER 10001:10001
ENTRYPOINT ["corqty-api"]
