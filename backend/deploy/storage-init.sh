#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
umask 077
test ! -e .env.storage || { echo '.env.storage already exists; nothing changed.' >&2; exit 1; }
test ! -e .storage || { echo '.storage already exists; inspect it before retrying.' >&2; exit 1; }
command -v openssl >/dev/null || { echo 'Install openssl first.' >&2; exit 1; }

temporary=$(mktemp -d .storage.init.XXXXXX)
cleanup() {
    rm -f "$temporary/environment" "$temporary/garage.toml"
    rmdir "$temporary" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
cp .env.storage.example "$temporary/environment"
rpc_secret=$(openssl rand -hex 32)
admin_token=$(openssl rand -hex 32)
cat >"$temporary/garage.toml" <<EOF
metadata_dir = "/var/lib/garage/meta"
data_dir = "/var/lib/garage/data"
db_engine = "sqlite"
metadata_auto_snapshot_interval = "6h"
replication_factor = 1
compression_level = 2
rpc_bind_addr = "[::]:3901"
rpc_public_addr = "127.0.0.1:3901"
rpc_secret = "$rpc_secret"

[s3_api]
s3_region = "garage"
api_bind_addr = "[::]:3900"
root_domain = ".s3.garage.internal"

[admin]
api_bind_addr = "127.0.0.1:3903"
admin_token = "$admin_token"
EOF
chmod 600 "$temporary/environment" "$temporary/garage.toml"
mv "$temporary/environment" .env.storage
mkdir .storage
mv "$temporary/garage.toml" .storage/garage.toml
echo 'Created .env.storage and .storage/garage.toml. Review capacity, then run: sh manage storage-up'
