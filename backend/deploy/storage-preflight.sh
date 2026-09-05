#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
fail() { echo "Storage preflight: $*" >&2; exit 1; }
test -f .env.storage || fail 'Run sh manage storage-init first.'
test -f .storage/garage.toml || fail '.storage/garage.toml is missing.'
test "$(stat -c '%a' .env.storage)" = 600 || fail 'Run chmod 600 .env.storage.'
test "$(stat -c '%a' .storage/garage.toml)" = 600 || fail 'Run chmod 600 .storage/garage.toml.'
set -a
. ./.env.storage
set +a
case "${GARAGE_IMAGE:-}" in *:v[0-9]*.[0-9]*.[0-9]*) ;; *) fail 'GARAGE_IMAGE must use a fixed release tag.' ;; esac
printf '%s\n' "${GARAGE_CAPACITY:-}" | grep -Eq '^[1-9][0-9]*(K|M|G|T)$' || fail 'GARAGE_CAPACITY must be a value such as 10G.'
case "${GARAGE_S3_PORT:-3900}" in ''|*[!0-9]*|0) fail 'GARAGE_S3_PORT must be a positive port.' ;; esac
test "${GARAGE_S3_PORT:-3900}" -le 65535 || fail 'GARAGE_S3_PORT is outside the port range.'
for name in GARAGE_ZONE GARAGE_BUCKET GARAGE_KEY_NAME; do
    eval "value=\${$name:-}"
    case "$value" in ''|*[!a-zA-Z0-9._-]*) fail "$name must use a safe identifier." ;; esac
done
grep -Eq '^rpc_secret = "[a-f0-9]{64}"$' .storage/garage.toml || fail 'Garage RPC secret is missing or invalid.'
docker network inspect crescentsphere-mail-transport >/dev/null 2>&1 || fail 'Start Stalwart first to create the private transport network.'
docker compose --project-name crescentsphere-storage --env-file .env.storage -f docker-compose.storage.yml config --quiet
echo 'Storage preflight passed. A single VPS still requires encrypted offsite backup.'
