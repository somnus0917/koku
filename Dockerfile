# syntax=docker/dockerfile:1

FROM rust:1.85-slim-bookworm AS builder

WORKDIR /build
COPY deploy/cargo-config.toml /usr/local/cargo/config.toml
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
    && cp /build/target/release/koku /build/koku

FROM debian:bookworm-slim AS runtime

ARG DEBIAN_MIRROR=
RUN if [ -n "${DEBIAN_MIRROR}" ]; then \
      sed -i \
        -e "s|http://deb.debian.org/debian-security|${DEBIAN_MIRROR}/debian-security|g" \
        -e "s|http://deb.debian.org/debian|${DEBIAN_MIRROR}/debian|g" \
        /etc/apt/sources.list.d/debian.sources; \
    fi \
    && apt-get -o Acquire::Retries=3 -o Acquire::ForceIPv4=true -o Acquire::http::Timeout=30 update \
    && apt-get -o Acquire::Retries=3 -o Acquire::ForceIPv4=true -o Acquire::http::Timeout=30 \
      install --yes --no-install-recommends ca-certificates curl sqlite3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 koku \
    && useradd --uid 10001 --gid koku --no-create-home --shell /usr/sbin/nologin koku \
    && mkdir -p /app/data \
    && chown -R koku:koku /app

COPY --from=builder /build/koku /usr/local/bin/koku

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
