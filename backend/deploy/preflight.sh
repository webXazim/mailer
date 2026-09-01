#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
fail() { echo "Preflight: $*" >&2; exit 1; }
test -f .env || fail 'Run sh manage production-init first.'
test "$(stat -c '%a' .env)" = 600 || fail 'Run chmod 600 .env.'
# Never render the resolved Compose config: it contains credentials.
set -a
. ./.env
set +a
for name in CLOUDFLARE_TUNNEL_TOKEN POSTGRES_PASSWORD NATS_PASSWORD EVENT_INGEST_TOKEN WEBHOOK_SIGNING_MASTER_KEY API_AWS_ACCESS_KEY_ID API_AWS_SECRET_ACCESS_KEY WORKER_AWS_ACCESS_KEY_ID WORKER_AWS_SECRET_ACCESS_KEY SES_EVENTS_QUEUE_URL SES_EVENTS_TOPIC_ARN SES_CONFIGURATION_SET ACCOUNT_EMAIL_FROM SIGNUP_TOKEN OBJECT_STORAGE_ENDPOINT OBJECT_STORAGE_BUCKET OBJECT_STORAGE_ACCESS_KEY_ID OBJECT_STORAGE_SECRET_ACCESS_KEY; do
    eval "value=\${$name:-}"
    test -n "$value" || fail "$name is required (see the top of .env)."
    case "$value" in *REPLACE*|*ACCOUNT_ID*|*change-me*|*mailer-development*) fail "$name still contains a placeholder." ;; esac
done
for name in POSTGRES_PASSWORD NATS_PASSWORD; do
    eval "value=\${$name}"
    test "${#value}" -ge 32 || fail "$name must contain at least 32 hex characters."
    case "$value" in *[!a-fA-F0-9]*) fail "$name must be URL-safe hex; generate with openssl rand -hex 32." ;; esac
done
for name in EVENT_INGEST_TOKEN WEBHOOK_SIGNING_MASTER_KEY SIGNUP_TOKEN; do
    eval "value=\${$name}"
    test "${#value}" -ge 32 || fail "$name must contain at least 32 characters."
done
for name in POSTGRES_USER POSTGRES_DB NATS_USER; do
    eval "value=\${$name:-mailer}"
    case "$value" in *[!a-zA-Z0-9_-]*) fail "$name must use letters, numbers, underscore or hyphen." ;; esac
done
test "${APP_ENV:-}" = production || fail 'APP_ENV must be production, including VPS tests.'
test "${CONSOLE_ORIGIN:-}" = https://mailer.crescentsphere.com || fail 'CONSOLE_ORIGIN must be https://mailer.crescentsphere.com.'
test "${DOMAIN_PROVIDER:-}" = ses || fail 'DOMAIN_PROVIDER must be ses.'
case "${OBJECT_STORAGE_PROVIDER:-}" in r2|s3) ;; *) fail 'Enable r2 or s3 object storage.' ;; esac
test "${TRUST_PROXY_HEADERS:-}" = true || fail 'TRUST_PROXY_HEADERS must be true for this private proxy topology.'
case "${FRONTEND_PORT:-0}" in ''|*[!0-9]*) fail 'FRONTEND_PORT must be 0 (automatic) or a port number.' ;; esac
test "${FRONTEND_PORT:-0}" -le 65535 || fail 'FRONTEND_PORT is outside the port range.'
case "${CARGO_BUILD_JOBS:-1}" in ''|*[!0-9]*|0) fail 'CARGO_BUILD_JOBS must be a positive integer.' ;; esac
command -v docker >/dev/null || fail 'Install Docker Engine and the Compose plugin.'
docker compose --project-name crescentsphere-mailer --env-file .env -f docker-compose.production.yml config --quiet
docker info >/dev/null 2>&1 || fail 'Docker Engine is unavailable to this user.'
echo 'Preflight passed. Provider credentials, public DNS and SES permissions still need a live test.'
