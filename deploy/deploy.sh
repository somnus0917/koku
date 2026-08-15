#!/usr/bin/env bash
# Production deploy entry point. Runtime data and secrets remain outside release sync.
set -Eeuo pipefail

readonly APP_DIR="${KOKU_APP_DIR:-/home/ubuntu/koku}"
readonly DATA_DIR="${KOKU_DATA_DIR:-${APP_DIR}/data}"
readonly CONFIG_ENV="${KOKU_DEPLOY_ENV:-${APP_DIR}/.env}"
readonly COMPOSE_FILE="${APP_DIR}/compose.production.yml"
readonly RELEASE_ENV="${APP_DIR}/.release.env"
readonly PREVIOUS_ENV="${APP_DIR}/.release.previous.env"
readonly CANDIDATE_ENV="${APP_DIR}/.release.next.env"
readonly LOCK_FILE="${DATA_DIR}/deploy.lock"
readonly DEPLOY_REF="${1:-}"
RELEASE_DIR=""

cleanup() {
    [[ -z "$RELEASE_DIR" ]] || rm -rf "$RELEASE_DIR"
}
trap cleanup EXIT

for command_name in curl docker flock rsync tar; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "required command is missing: $command_name" >&2
        exit 1
    fi
done
if ! docker compose version >/dev/null 2>&1; then
    echo "Docker Compose v2 is required" >&2
    exit 1
fi
if [[ ! "$DEPLOY_REF" =~ ^[0-9a-f]{40}$ ]]; then
    echo "deployment revision must be a full lowercase Git SHA" >&2
    exit 2
fi

mkdir -p "$DATA_DIR/backups"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
    echo "a Koku deployment is already running" >&2
    exit 1
fi

RELEASE_DIR="$(mktemp -d "${DATA_DIR}/release.XXXXXX")"
curl --fail --location \
    --retry 3 \
    --retry-delay 3 \
    --connect-timeout 15 \
    --max-time 180 \
    "https://codeload.github.com/somnus0917/koku/tar.gz/${DEPLOY_REF}" \
    | tar -xzf - --strip-components=1 -C "$RELEASE_DIR"

# Delete stale source files while preserving all host-owned runtime state.
rsync -a --delete \
    --exclude='.env' \
    --exclude='.release.env' \
    --exclude='.release.previous.env' \
    --exclude='.release.next.env' \
    --exclude='.deploy-revision' \
    --exclude='.git/' \
    --exclude='data/' \
    "$RELEASE_DIR/" "$APP_DIR/"
chmod 700 "$APP_DIR/deploy/deploy.sh"
printf '%s\n' "$DEPLOY_REF" > "$APP_DIR/.deploy-revision"

for required_file in "$COMPOSE_FILE" "$CONFIG_ENV"; do
    if [[ ! -f "$required_file" ]]; then
        echo "required deployment file is missing: $required_file" >&2
        exit 1
    fi
done

umask 077
printf 'KOKU_API_IMAGE=koku-api:%s\nKOKU_WEB_IMAGE=koku-web:%s\n' \
    "$DEPLOY_REF" "$DEPLOY_REF" > "$CANDIDATE_ENV"

compose_with_release() {
    local image_env=$1
    shift
    docker compose \
        --project-name koku \
        --env-file "$CONFIG_ENV" \
        --env-file "$image_env" \
        --file "$COMPOSE_FILE" \
        "$@"
}

backup_database() {
    if ! docker inspect koku-api >/dev/null 2>&1; then
        return
    fi
    if ! docker exec koku-api test -f /app/data/koku.db; then
        return
    fi

    local timestamp
    timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
    docker exec koku-api mkdir -p /app/data/backups
    docker exec koku-api sqlite3 /app/data/koku.db \
        '.timeout 5000' \
        ".backup /app/data/backups/koku-${timestamp}.db"
    echo "SQLite backup created: data/backups/koku-${timestamp}.db"

    # 保留最近 10 份备份，更早的自动清理，防止无限累积。
    ls -1t "$DATA_DIR/backups"/koku-*.db 2>/dev/null | tail -n +11 | xargs -r rm -f
}

rollback() {
    if [[ ! -f "$RELEASE_ENV" ]]; then
        echo "no previous release is available for automatic rollback" >&2
        return 1
    fi
    echo "deployment failed; restoring the previous image pair" >&2
    compose_with_release "$RELEASE_ENV" up \
        --detach \
        --remove-orphans \
        --wait \
        --wait-timeout 90
}

backup_database
compose_with_release "$CANDIDATE_ENV" config --quiet
compose_with_release "$CANDIDATE_ENV" build

if ! compose_with_release "$CANDIDATE_ENV" up \
    --detach \
    --remove-orphans \
    --wait \
    --wait-timeout 90; then
    rollback || true
    rm -f "$CANDIDATE_ENV"
    exit 1
fi

if [[ -f "$RELEASE_ENV" ]]; then
    cp "$RELEASE_ENV" "$PREVIOUS_ENV"
fi
mv "$CANDIDATE_ENV" "$RELEASE_ENV"
echo "Koku deployment is healthy at https://$(sed -n 's/^KOKU_DOMAIN=//p' "$CONFIG_ENV")"
echo "Deployed source revision: $DEPLOY_REF"
compose_with_release "$RELEASE_ENV" ps
