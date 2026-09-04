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

: "${STALWART_HOSTNAME:?Set STALWART_HOSTNAME}"
: "${STALWART_IPV4:?Set STALWART_IPV4}"

for command_name in dig nc; do
    command -v "$command_name" >/dev/null 2>&1 || {
        echo "$command_name is required for the live network check" >&2
        exit 1
    }
done

forward=$(dig +short @1.1.1.1 A "$STALWART_HOSTNAME" | tr -d '\r')
printf '%s\n' "$forward" | grep -Fx "$STALWART_IPV4" >/dev/null || {
    echo "Forward DNS mismatch: $STALWART_HOSTNAME does not resolve to $STALWART_IPV4" >&2
    printf 'Resolver returned:\n%s\n' "$forward" >&2
    exit 1
}

reverse=$(dig +short @1.1.1.1 -x "$STALWART_IPV4" | tr -d '\r' | sed 's/\.$//')
test "$reverse" = "$STALWART_HOSTNAME" || {
    echo "PTR mismatch: $STALWART_IPV4 resolves to ${reverse:-nothing}, expected $STALWART_HOSTNAME" >&2
    exit 1
}

nc -4 -z -w 10 gmail-smtp-in.l.google.com 25 || {
    echo "Outbound IPv4 TCP 25 is unavailable" >&2
    exit 1
}

echo "Forward DNS, PTR, and outbound IPv4 TCP 25 passed."

