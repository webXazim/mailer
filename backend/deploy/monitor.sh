#!/bin/sh
set -eu

failure=
if ! sh "$(dirname "$0")/healthcheck.sh"; then
    failure='public health or queue progress check failed'
fi

disk_used=$(df -P "$(dirname "$0")/../.." | awk 'NR==2 { gsub(/%/,"",$5); print $5 }')
if [ -z "$failure" ] && { [ -z "$disk_used" ] || [ "$disk_used" -ge "${DISK_ALERT_PERCENT:-85}" ]; }; then
    failure="filesystem usage is ${disk_used:-unknown}%"
fi

if [ -z "$failure" ] && [ "${DELIVERY_PROVIDER:-ses}" = smtp ]; then
    certificate=$(mktemp)
    trap 'rm -f "$certificate"' EXIT HUP INT TERM
    if ! timeout 15 openssl s_client -connect "${MTA_PUBLIC_HOST:?MTA_PUBLIC_HOST is required}:465" \
        -servername "$MTA_PUBLIC_HOST" -verify_hostname "$MTA_PUBLIC_HOST" \
        -verify_return_error </dev/null 2>/dev/null \
        | openssl x509 -outform PEM >"$certificate"; then
        failure='SMTP TLS connection or hostname verification failed'
    elif ! openssl x509 -in "$certificate" -checkend "${TLS_ALERT_SECONDS:-604800}" -noout >/dev/null; then
        failure='SMTP TLS certificate expires within the alert window'
    fi
    rm -f "$certificate"
    trap - EXIT HUP INT TERM
fi

test -n "$failure" || exit 0

message="CrescentSphere Mailer production monitor: $failure"
logger -t crescentsphere-mailer "$message"
if [ -n "${ALERT_WEBHOOK_URL:-}" ]; then
    curl --fail --silent --show-error --max-time 10 \
        --header 'Content-Type: application/json' \
        --data "{\"text\":\"$message\"}" \
        "$ALERT_WEBHOOK_URL" >/dev/null || true
fi
exit 1
