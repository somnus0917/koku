FROM rust:1.85-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl sqlite3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 koku \
    && useradd --uid 10001 --gid koku --no-create-home --shell /usr/sbin/nologin koku \
    && mkdir -p /app/data \
    && chown -R koku:koku /app

COPY --from=builder /build/target/release/koku /usr/local/bin/koku

WORKDIR /app
USER koku

ENV KOKU_HOST=0.0.0.0 \
    KOKU_PORT=8080 \
    KOKU_DB_PATH=/app/data/koku.db \
    KOKU_SEED_DEMO=false

EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=4 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/api/health >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/koku"]
