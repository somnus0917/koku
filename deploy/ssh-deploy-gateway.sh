#!/usr/bin/env bash
# Forced command for Koku's restricted GitHub Actions SSH key.
set -Eeuo pipefail

readonly DEPLOY_SCRIPT="/home/ubuntu/koku/deploy/deploy.sh"
readonly RECEIVED_COMMAND="${SSH_ORIGINAL_COMMAND:-}"

if [[ "$RECEIVED_COMMAND" =~ ^deploy\ ([0-9a-f]{40})$ ]]; then
    readonly DEPLOY_SHA="${BASH_REMATCH[1]}"
    exec sudo -n -u ubuntu -- "$DEPLOY_SCRIPT" "$DEPLOY_SHA"
fi

echo 'This SSH key only permits:' >&2
echo '  deploy <40-character lowercase Git SHA>' >&2
exit 126
