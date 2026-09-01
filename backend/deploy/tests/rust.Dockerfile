# syntax=docker/dockerfile:1
# Build with backend/ as the context. No provider credentials are used.
FROM rust:1.97-bookworm
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo test --locked --release --jobs 1 --workspace
