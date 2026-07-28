FROM rust:1.96-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --locked --bins
CMD ["cargo", "run", "--locked", "-p", "corqty-api"]
