FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --package cs-mail-api --package cs-mail-worker

FROM debian:bookworm-slim AS api
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 mailer
COPY --from=build /src/target/release/cs-mail-api /usr/local/bin/cs-mail-api
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
COPY --from=build /src/target/release/cs-mail-worker /usr/local/bin/cs-mail-worker
USER mailer
ENTRYPOINT ["/usr/local/bin/cs-mail-worker"]
