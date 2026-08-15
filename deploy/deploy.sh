#!/usr/bin/env bash
set -Eeuo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <api-image> <web-image>" >&2
    exit 2
fi

api_image=$1
web_image=$2
deploy_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
compose_file="$deploy_dir/compose.production.yml"
config_env="$deploy_dir/.env"
release_env="$deploy_dir/.release.env"
candidate_env="$deploy_dir/.release.next.env"

for command_name in docker; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        echo "required command is missing: $command_name" >&2
        exit 1
    fi
done

if ! docker compose version >/dev/null 2>&1; then
    echo "Docker Compose v2 is required" >&2
    exit 1
fi

for required_file in "$compose_file" "$config_env"; do
    if [[ ! -f "$required_file" ]]; then
        echo "required deployment file is missing: $required_file" >&2
        exit 1
    fi
done

mkdir -p "$deploy_dir/data/backups"
umask 077
printf 'KOKU_API_IMAGE=%s\nKOKU_WEB_IMAGE=%s\n' "$api_image" "$web_image" >"$candidate_env"

compose_with_release() {
    local image_env=$1
    shift
    docker compose \
        --project-name koku \
        --env-file "$config_env" \
        --env-file "$image_env" \
        --file "$compose_file" \
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
    timestamp=$(date -u +%Y%m%dT%H%M%SZ)
    docker exec koku-api mkdir -p /app/data/backups
    docker exec koku-api sqlite3 /app/data/koku.db \
        ".timeout 5000" \
        ".backup /app/data/backups/koku-$timestamp.db"
    echo "SQLite backup created: data/backups/koku-$timestamp.db"
}

rollback() {
    if [[ ! -f "$release_env" ]]; then
        echo "No previous release is available for automatic rollback" >&2
        return 1
    fi

    echo "Deployment failed; restoring the previous image pair" >&2
    compose_with_release "$release_env" up \
        --detach \
        --remove-orphans \
        --wait \
        --wait-timeout 90
}

backup_database
compose_with_release "$candidate_env" config --quiet
if ! compose_with_release "$candidate_env" pull; then
    rm -f "$candidate_env"
    exit 1
fi

if ! compose_with_release "$candidate_env" up \
    --detach \
    --remove-orphans \
    --wait \
    --wait-timeout 90; then
    rollback || true
    rm -f "$candidate_env"
    exit 1
fi

mv "$candidate_env" "$release_env"
echo "Koku deployment is healthy at https://$(sed -n 's/^KOKU_DOMAIN=//p' "$config_env")"
