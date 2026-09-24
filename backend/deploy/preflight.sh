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
for name in CLOUDFLARE_TUNNEL_TOKEN TURNSTILE_SITE_KEY TURNSTILE_SECRET_KEY POSTGRES_PASSWORD NATS_PASSWORD WEBHOOK_SIGNING_MASTER_KEY OBJECT_STORAGE_ENDPOINT OBJECT_STORAGE_BUCKET OBJECT_STORAGE_ACCESS_KEY_ID OBJECT_STORAGE_SECRET_ACCESS_KEY; do
    eval "value=\${$name:-}"
    test -n "$value" || fail "$name is required (see the top of .env)."
    case "$value" in *REPLACE*|*ACCOUNT_ID*|*change-me*|*mailer-development*) fail "$name still contains a placeholder." ;; esac
done
for name in STALWART_API_URL STALWART_API_TOKEN MTA_PUBLIC_HOST MTA_PUBLIC_IPV4 SMTP_HOST SMTP_USERNAME SMTP_PASSWORD SMTP_HELO_NAME STALWART_WEBHOOK_TOKEN STALWART_WEBHOOK_SIGNING_KEY; do
        eval "value=\${$name:-}"
        test -n "$value" || fail "$name is required for independent mail delivery."
        case "$value" in *REPLACE*|*change-me*) fail "$name still contains a placeholder." ;; esac
done
case "${STALWART_API_URL}" in http://*|https://*) ;; *) fail 'STALWART_API_URL must use http:// or https://.' ;; esac
test "${#STALWART_API_TOKEN}" -ge 32 || fail 'STALWART_API_TOKEN must contain at least 32 characters.'
case "${MTA_RETURN_PATH_PREFIX:-bounce}" in ''|*[!a-zA-Z0-9-]*) fail 'MTA_RETURN_PATH_PREFIX must be a DNS label.' ;; esac
case "${SMTP_PORT:-465}" in ''|*[!0-9]*|0) fail 'SMTP_PORT must be a positive port number.' ;; esac
test "${SMTP_PORT:-465}" -le 65535 || fail 'SMTP_PORT is outside the port range.'
case "${SMTP_SECURITY:-implicit_tls}" in implicit_tls|starttls) ;; *) fail 'SMTP_SECURITY must be implicit_tls or starttls.' ;; esac
case "${SMTP_GATEWAY_ENABLED:-false}" in
    true)
        gateway_secret=${SMTP_GATEWAY_SHARED_SECRET:-}
        test "${#gateway_secret}" -ge 32 || fail 'SMTP_GATEWAY_SHARED_SECRET must be at least 32 characters.'
        test -f "${SMTP_GATEWAY_TLS_DIR:-./secrets/smtp}/fullchain.pem" || fail 'SMTP gateway certificate is missing.'
        test -f "${SMTP_GATEWAY_TLS_DIR:-./secrets/smtp}/privkey.pem" || fail 'SMTP gateway private key is missing.'
        command -v openssl >/dev/null || fail 'OpenSSL is required to check SMTP gateway TLS.'
        openssl x509 -in "${SMTP_GATEWAY_TLS_DIR:-./secrets/smtp}/fullchain.pem" -noout -checkhost smtp.mailer.crescentsphere.com >/dev/null || fail 'SMTP gateway certificate must cover smtp.mailer.crescentsphere.com.'
        openssl x509 -in "${SMTP_GATEWAY_TLS_DIR:-./secrets/smtp}/fullchain.pem" -noout -checkend 604800 >/dev/null || fail 'SMTP gateway certificate expires within 7 days.'
        bind_ip=${MAILER_SMTP_BIND_IP:-127.0.0.1}
        case "$bind_ip" in 0.0.0.0|::|'') fail 'Bind the SMTP gateway to a specific IPv4 address.' ;; esac
        for name in MAILER_SMTP_IMPLICIT_PORT MAILER_SMTP_STARTTLS_PORT; do
            eval "value=\${$name:-}"
            case "$value" in ''|*[!0-9]*|0) fail "$name must be a valid port." ;; esac
            test "$value" -le 65535 || fail "$name is outside the port range."
        done
        test "${MAILER_SMTP_IMPLICIT_PORT}" != "${MAILER_SMTP_STARTTLS_PORT}" || fail 'SMTP gateway ports must differ.'
        if [ "$bind_ip" = "${MTA_PUBLIC_IPV4:-152.53.178.165}" ] && { [ "$MAILER_SMTP_IMPLICIT_PORT" = 465 ] || [ "$MAILER_SMTP_STARTTLS_PORT" = 587 ]; }; then
            fail 'Stalwart owns 465/587 on the primary mail IP. Use alternate ports or a second IP.'
        fi
        export COMPOSE_PROFILES=smtp
        ;;
    false) unset COMPOSE_PROFILES ;;
    *) fail 'SMTP_GATEWAY_ENABLED must be true or false.' ;;
esac
case "${SMTP_TIMEOUT_SECONDS:-30}" in ''|*[!0-9]*|0) fail 'SMTP_TIMEOUT_SECONDS must be a positive integer.' ;; esac
test "${#STALWART_WEBHOOK_TOKEN}" -ge 32 || fail 'STALWART_WEBHOOK_TOKEN must contain at least 32 characters.'
test "${#STALWART_WEBHOOK_SIGNING_KEY}" -ge 32 || fail 'STALWART_WEBHOOK_SIGNING_KEY must contain at least 32 characters.'
test "$STALWART_WEBHOOK_TOKEN" != "$STALWART_WEBHOOK_SIGNING_KEY" || fail 'Use different Stalwart webhook bearer and HMAC secrets.'
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
for name in WEBHOOK_SIGNING_MASTER_KEY TURNSTILE_SECRET_KEY; do
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
case "${BACKUP_OBJECT_STORAGE:-false}" in
    true)
        test -n "${BACKUP_RCLONE_REMOTE:-}" || fail 'BACKUP_RCLONE_REMOTE is required when BACKUP_OBJECT_STORAGE=true.'
        test -n "${OBJECT_STORAGE_BACKUP_ENDPOINT:-}" || fail 'OBJECT_STORAGE_BACKUP_ENDPOINT is required when BACKUP_OBJECT_STORAGE=true.'
        command -v rclone >/dev/null || fail 'Install rclone when BACKUP_OBJECT_STORAGE=true.'
        ;;
    false) ;;
    *) fail 'BACKUP_OBJECT_STORAGE must be true or false.' ;;
esac
test "${TRUST_PROXY_HEADERS:-}" = true || fail 'TRUST_PROXY_HEADERS must be true for this private proxy topology.'
case "${FRONTEND_PORT:-0}" in ''|*[!0-9]*) fail 'FRONTEND_PORT must be 0 (automatic) or a port number.' ;; esac
test "${FRONTEND_PORT:-0}" -le 65535 || fail 'FRONTEND_PORT is outside the port range.'
case "${CARGO_BUILD_JOBS:-1}" in ''|*[!0-9]*|0) fail 'CARGO_BUILD_JOBS must be a positive integer.' ;; esac
command -v docker >/dev/null || fail 'Install Docker Engine and the Compose plugin.'
docker compose --project-name crescentsphere-mailer --env-file .env -f docker-compose.production.yml config --quiet
docker info >/dev/null 2>&1 || fail 'Docker Engine is unavailable to this user.'
docker network inspect crescentsphere-mail-transport >/dev/null 2>&1 || fail 'Start the independent Stalwart stack before deploying Mailer.'
echo "Preflight passed for independent SMTP delivery. Provider credentials and public DNS still need a live test."
