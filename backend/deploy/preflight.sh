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
for name in CLOUDFLARE_TUNNEL_TOKEN TURNSTILE_SITE_KEY TURNSTILE_SECRET_KEY POSTGRES_PASSWORD NATS_PASSWORD EVENT_INGEST_TOKEN WEBHOOK_SIGNING_MASTER_KEY OBJECT_STORAGE_ENDPOINT OBJECT_STORAGE_BUCKET OBJECT_STORAGE_ACCESS_KEY_ID OBJECT_STORAGE_SECRET_ACCESS_KEY; do
    eval "value=\${$name:-}"
    test -n "$value" || fail "$name is required (see the top of .env)."
    case "$value" in *REPLACE*|*ACCOUNT_ID*|*change-me*|*mailer-development*) fail "$name still contains a placeholder." ;; esac
done
case "${DOMAIN_PROVIDER:-}" in
    ses)
        for name in API_AWS_ACCESS_KEY_ID API_AWS_SECRET_ACCESS_KEY; do
            eval "value=\${$name:-}"
            test -n "$value" || fail "$name is required when DOMAIN_PROVIDER=ses."
            case "$value" in *REPLACE*|*change-me*) fail "$name still contains a placeholder." ;; esac
        done
        ;;
    stalwart)
        for name in STALWART_API_URL STALWART_API_TOKEN MTA_PUBLIC_HOST MTA_PUBLIC_IPV4; do
            eval "value=\${$name:-}"
            test -n "$value" || fail "$name is required when DOMAIN_PROVIDER=stalwart."
            case "$value" in *REPLACE*|*change-me*) fail "$name still contains a placeholder." ;; esac
        done
        case "${STALWART_API_URL}" in http://*|https://*) ;; *) fail 'STALWART_API_URL must use http:// or https://.' ;; esac
        test "${#STALWART_API_TOKEN}" -ge 32 || fail 'STALWART_API_TOKEN must contain at least 32 characters.'
        case "${MTA_RETURN_PATH_PREFIX:-bounce}" in ''|*[!a-zA-Z0-9-]*) fail 'MTA_RETURN_PATH_PREFIX must be a DNS label.' ;; esac
        ;;
    *) fail 'DOMAIN_PROVIDER must be ses or stalwart.' ;;
esac
case "${DELIVERY_PROVIDER:-ses}" in
    ses)
        for name in WORKER_AWS_ACCESS_KEY_ID WORKER_AWS_SECRET_ACCESS_KEY SES_EVENTS_QUEUE_URL SES_EVENTS_TOPIC_ARN SES_CONFIGURATION_SET; do
            eval "value=\${$name:-}"
            test -n "$value" || fail "$name is required when DELIVERY_PROVIDER=ses."
            case "$value" in *REPLACE*|*ACCOUNT_ID*|*change-me*) fail "$name still contains a placeholder." ;; esac
        done
        ;;
    smtp)
        for name in SMTP_HOST SMTP_USERNAME SMTP_PASSWORD SMTP_HELO_NAME STALWART_WEBHOOK_TOKEN STALWART_WEBHOOK_SIGNING_KEY; do
            eval "value=\${$name:-}"
            test -n "$value" || fail "$name is required when DELIVERY_PROVIDER=smtp."
            case "$value" in *REPLACE*|*change-me*) fail "$name still contains a placeholder." ;; esac
        done
        case "${SMTP_PORT:-465}" in ''|*[!0-9]*|0) fail 'SMTP_PORT must be a positive port number.' ;; esac
        test "${SMTP_PORT:-465}" -le 65535 || fail 'SMTP_PORT is outside the port range.'
        case "${SMTP_SECURITY:-implicit_tls}" in implicit_tls|starttls) ;; *) fail 'SMTP_SECURITY must be implicit_tls or starttls.' ;; esac
        case "${SMTP_TIMEOUT_SECONDS:-30}" in ''|*[!0-9]*|0) fail 'SMTP_TIMEOUT_SECONDS must be a positive integer.' ;; esac
        test "${#STALWART_WEBHOOK_TOKEN}" -ge 32 || fail 'STALWART_WEBHOOK_TOKEN must contain at least 32 characters.'
        test "${#STALWART_WEBHOOK_SIGNING_KEY}" -ge 32 || fail 'STALWART_WEBHOOK_SIGNING_KEY must contain at least 32 characters.'
        test "$STALWART_WEBHOOK_TOKEN" != "$STALWART_WEBHOOK_SIGNING_KEY" || fail 'Use different Stalwart webhook bearer and HMAC secrets.'
        if test -n "${WORKER_AWS_ACCESS_KEY_ID:-}${WORKER_AWS_SECRET_ACCESS_KEY:-}${SES_EVENTS_QUEUE_URL:-}${SES_EVENTS_TOPIC_ARN:-}${SES_CONFIGURATION_SET:-}"; then
            for name in WORKER_AWS_ACCESS_KEY_ID WORKER_AWS_SECRET_ACCESS_KEY SES_EVENTS_QUEUE_URL SES_EVENTS_TOPIC_ARN SES_CONFIGURATION_SET; do
                eval "value=\${$name:-}"
                test -n "$value" || fail "Set all SES worker/event variables to drain and observe existing SES mail, or clear all of them. Missing $name."
            done
        fi
        ;;
    *) fail 'DELIVERY_PROVIDER must be ses or smtp.' ;;
esac
case "${AUTH_EMAIL_DELIVERY_ENABLED:-false}" in
    true)
        test -n "${ACCOUNT_EMAIL_FROM:-}" || fail 'ACCOUNT_EMAIL_FROM is required when AUTH_EMAIL_DELIVERY_ENABLED=true.'
        case "${ACCOUNT_EMAIL_API_KEY:-}" in cs_live_*) ;; *) fail 'A live ACCOUNT_EMAIL_API_KEY is required when AUTH_EMAIL_DELIVERY_ENABLED=true.' ;; esac
        ;;
    false) ;;
    *) fail 'AUTH_EMAIL_DELIVERY_ENABLED must be true or false.' ;;
esac
if { test -n "${CLOUDFLARE_OAUTH_CLIENT_ID:-}" && test -z "${CLOUDFLARE_OAUTH_CLIENT_SECRET:-}"; } ||
   { test -z "${CLOUDFLARE_OAUTH_CLIENT_ID:-}" && test -n "${CLOUDFLARE_OAUTH_CLIENT_SECRET:-}"; }; then
    fail 'Set both CLOUDFLARE_OAUTH_CLIENT_ID and CLOUDFLARE_OAUTH_CLIENT_SECRET, or leave both empty.'
fi
for name in POSTGRES_PASSWORD NATS_PASSWORD; do
    eval "value=\${$name}"
    test "${#value}" -ge 32 || fail "$name must contain at least 32 hex characters."
    case "$value" in *[!a-fA-F0-9]*) fail "$name must be URL-safe hex; generate with openssl rand -hex 32." ;; esac
done
for name in EVENT_INGEST_TOKEN WEBHOOK_SIGNING_MASTER_KEY TURNSTILE_SECRET_KEY; do
    eval "value=\${$name}"
    test "${#value}" -ge 32 || fail "$name must contain at least 32 characters."
done
test "${#TURNSTILE_SECRET_KEY}" -ge 20 || fail 'TURNSTILE_SECRET_KEY is too short.'
for name in POSTGRES_USER POSTGRES_DB NATS_USER; do
    eval "value=\${$name:-mailer}"
    case "$value" in *[!a-zA-Z0-9_-]*) fail "$name must use letters, numbers, underscore or hyphen." ;; esac
done
test "${APP_ENV:-}" = production || fail 'APP_ENV must be production, including VPS tests.'
test "${CONSOLE_ORIGIN:-}" = https://mailer.crescentsphere.com || fail 'CONSOLE_ORIGIN must be https://mailer.crescentsphere.com.'
case "${OBJECT_STORAGE_PROVIDER:-}" in r2|s3) ;; *) fail 'Enable r2 or s3 object storage.' ;; esac
test "${TRUST_PROXY_HEADERS:-}" = true || fail 'TRUST_PROXY_HEADERS must be true for this private proxy topology.'
case "${FRONTEND_PORT:-0}" in ''|*[!0-9]*) fail 'FRONTEND_PORT must be 0 (automatic) or a port number.' ;; esac
test "${FRONTEND_PORT:-0}" -le 65535 || fail 'FRONTEND_PORT is outside the port range.'
case "${CARGO_BUILD_JOBS:-1}" in ''|*[!0-9]*|0) fail 'CARGO_BUILD_JOBS must be a positive integer.' ;; esac
command -v docker >/dev/null || fail 'Install Docker Engine and the Compose plugin.'
docker compose --project-name crescentsphere-mailer --env-file .env -f docker-compose.production.yml config --quiet
docker info >/dev/null 2>&1 || fail 'Docker Engine is unavailable to this user.'
if test "${DOMAIN_PROVIDER:-}" = stalwart; then
    docker network inspect crescentsphere-mail-transport >/dev/null 2>&1 || fail 'Start the independent Stalwart stack before deploying Mailer.'
fi
echo "Preflight passed for ${DELIVERY_PROVIDER:-ses} delivery. Provider credentials and public DNS still need a live test."
