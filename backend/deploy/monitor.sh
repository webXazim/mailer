#!/bin/sh
set -eu

if sh "$(dirname "$0")/healthcheck.sh"; then
    exit 0
fi

message='CrescentSphere Mailer production health check failed'
logger -t crescentsphere-mailer "$message"
if [ -n "${ALERT_WEBHOOK_URL:-}" ]; then
    curl --fail --silent --show-error --max-time 10 \
        --header 'Content-Type: application/json' \
        --data '{"text":"CrescentSphere Mailer production health check failed"}' \
        "$ALERT_WEBHOOK_URL" >/dev/null || true
fi
exit 1
