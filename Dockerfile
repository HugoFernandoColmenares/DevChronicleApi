FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --bin devchronicle-api --bin seed

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 chronicle \
    && mkdir -p /data \
    && chown chronicle:chronicle /data

COPY --from=build /src/target/release/devchronicle-api /usr/local/bin/devchronicle-api
COPY --from=build /src/target/release/seed /usr/local/bin/seed
RUN printf '%s\n' \
    '#!/bin/sh' \
    'set -eu' \
    'mkdir -p /data' \
    'chown chronicle:chronicle /data' \
    'exec runuser -u chronicle -- "$@"' \
    > /usr/local/bin/entrypoint.sh \
    && chmod 755 /usr/local/bin/entrypoint.sh

EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --retries=5 CMD curl -fsS http://127.0.0.1:8080/api/v1/health || exit 1
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["/usr/local/bin/devchronicle-api"]
