#!/bin/sh
set -eu

cd "$(dirname "$0")/../.."

test -f .env.stalwart || {
    echo ".env.stalwart is required; run: sh manage stalwart-init" >&2
    exit 1
}

set -a
. ./.env.stalwart
set +a

: "${STALWART_IMAGE:?Set STALWART_IMAGE}"
: "${STALWART_RECOVERY_ADMIN:?Set STALWART_RECOVERY_ADMIN}"
: "${STALWART_HOSTNAME:?Set STALWART_HOSTNAME}"
: "${STALWART_IPV4:?Set STALWART_IPV4}"

case "$STALWART_IMAGE" in
    *:latest|*:edge)
        echo "STALWART_IMAGE must use a pinned version or digest, not latest/edge" >&2
        exit 1
        ;;
esac

case "$STALWART_RECOVERY_ADMIN" in
    *:*) ;;
    *)
        echo "STALWART_RECOVERY_ADMIN must use username:password format" >&2
        exit 1
        ;;
esac

case "${STALWART_ADMIN_PORT:-8088}" in
    ''|*[!0-9]*)
        echo "STALWART_ADMIN_PORT must be numeric" >&2
        exit 1
        ;;
esac

case "$STALWART_PUBLIC_URL" in
    "https://$STALWART_HOSTNAME"|"https://$STALWART_HOSTNAME/" ) ;;
    *)
        echo "STALWART_PUBLIC_URL must be https://$STALWART_HOSTNAME" >&2
        exit 1
        ;;
esac

docker compose \
    --project-name crescentsphere-stalwart \
    --env-file .env.stalwart \
    -f docker-compose.stalwart.yml \
    config --quiet

echo "Stalwart configuration passed. Run the live DNS/network check on the VPS next."

