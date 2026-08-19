#!/bin/sh
set -eu

cd "$(dirname "$0")/../.."
test -f .env
mode=$(stat -c '%a' .env)
test "$mode" = "600" || { echo ".env must have mode 0600" >&2; exit 1; }

if grep -Eq '(^|=)(REPLACE_|.*change-me|mailer-development)' .env; then
    echo ".env still contains development or placeholder values" >&2
    exit 1
fi

set -a
. ./.env
set +a

test "${APP_ENV:-}" = production
test "${CONSOLE_ORIGIN:-}" = https://mailer.crescentsphere.com
test "${TRUST_PROXY_HEADERS:-}" = true
test -n "${API_IMAGE:-}"
test -n "${WORKER_IMAGE:-}"
test -n "${FRONTEND_IMAGE:-}"

getent hosts api.mailer.crescentsphere.com >/dev/null
getent hosts mailer.crescentsphere.com >/dev/null

docker compose --env-file .env -f docker-compose.yml -f docker-compose.production.yml config --quiet
caddy validate --config backend/deploy/Caddyfile

echo "Preflight passed. No services were started."
