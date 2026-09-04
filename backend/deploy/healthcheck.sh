#!/bin/sh
set -eu

: "${PUBLIC_API_URL:=https://mailer.crescentsphere.com/api}"
: "${PUBLIC_CONSOLE_URL:=https://mailer.crescentsphere.com}"
curl --fail --silent --show-error --max-time 10 "$PUBLIC_API_URL/healthz" >/dev/null
curl --fail --silent --show-error --max-time 10 "$PUBLIC_API_URL/readyz" >/dev/null
curl --fail --silent --show-error --max-time 10 "$PUBLIC_API_URL/operationalz" >/dev/null
curl --fail --silent --show-error --max-time 10 "$PUBLIC_CONSOLE_URL/healthz" >/dev/null

status=$(curl --silent --output /dev/null --write-out '%{http_code}' --max-time 10 \
    "$PUBLIC_API_URL/internal/v1/ses/events")
test "$status" = "404"

status=$(curl --silent --output /dev/null --write-out "%{http_code}" --max-time 10 "$PUBLIC_CONSOLE_URL/internal/v1/ses/events")
test "$status" = "404"
status=$(curl --silent --output /dev/null --write-out "%{http_code}" --max-time 10 "$PUBLIC_CONSOLE_URL/internal/v1/stalwart/events")
test "$status" = "404"
echo "Public readiness, worker progress, queues, and private-route checks passed."
