# syntax=docker/dockerfile:1
FROM rust:1.97-bookworm AS build
WORKDIR /src
ARG CARGO_BUILD_JOBS=1
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release --jobs "$CARGO_BUILD_JOBS" --package cs-mail-api --package cs-mail-worker \
    && mkdir -p /out \
    && cp target/release/cs-mail-api target/release/cs-mail-worker /out/

FROM debian:bookworm-slim AS api
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 mailer
COPY --from=build /out/cs-mail-api /usr/local/bin/cs-mail-api
USER mailer
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:8080/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/cs-mail-api"]

FROM debian:bookworm-slim AS worker
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 mailer
COPY --from=build /out/cs-mail-worker /usr/local/bin/cs-mail-worker
USER mailer
ENTRYPOINT ["/usr/local/bin/cs-mail-worker"]
