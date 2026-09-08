#!/bin/sh
# Add newly introduced production variables without changing existing values.
set -eu
cd "$(dirname "$0")/../.."
umask 077

test -f .env || { echo 'Run sh manage production-init first.' >&2; exit 1; }
test -f .env.production.example || { echo '.env.production.example is missing.' >&2; exit 1; }

temporary=$(mktemp .env.upgrade.XXXXXX)
trap 'rm -f "$temporary"' EXIT HUP INT TERM
cp .env "$temporary"
added=0
removed=0

for name in API_AWS_ACCESS_KEY_ID API_AWS_SECRET_ACCESS_KEY API_AWS_SESSION_TOKEN WORKER_AWS_ACCESS_KEY_ID WORKER_AWS_SECRET_ACCESS_KEY WORKER_AWS_SESSION_TOKEN AWS_REGION SES_CONFIGURATION_SET SES_EVENTS_QUEUE_URL SES_EVENTS_TOPIC_ARN DELIVERY_PROVIDER DOMAIN_PROVIDER EVENT_INGEST_TOKEN; do
    if grep -q "^${name}=" "$temporary"; then
        sed -i "/^${name}=/d" "$temporary"
        printf 'Removed obsolete %s\n' "$name"
        removed=$((removed + 1))
    fi
done

while IFS= read -r line || test -n "$line"; do
    case "$line" in
        [A-Za-z_][A-Za-z0-9_]*=*)
            name=${line%%=*}
            if ! grep -q "^${name}=" "$temporary"; then
                printf '%s\n' "$line" >>"$temporary"
                printf 'Added %s\n' "$name"
                added=$((added + 1))
            fi
            ;;
    esac
done <.env.production.example

chmod 600 "$temporary"
mv "$temporary" .env
trap - EXIT HUP INT TERM
echo "Environment upgrade complete: $added missing variable(s) added; $removed obsolete provider variable(s) removed."
