#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
umask 077
if [ -e .env ]; then
    echo '.env already exists; it was not changed. Edit it or back it up before initializing.' >&2
    exit 1
fi
command -v openssl >/dev/null || { echo 'Install openssl first.' >&2; exit 1; }
temporary=$(mktemp .env.init.XXXXXX)
trap 'rm -f "$temporary"' EXIT HUP INT TERM
cp .env.production.example "$temporary"
for name in POSTGRES_PASSWORD NATS_PASSWORD EVENT_INGEST_TOKEN WEBHOOK_SIGNING_MASTER_KEY SIGNUP_TOKEN; do
    value=$(openssl rand -hex 32)
    sed -i "s/^$name=\$/$name=$value/" "$temporary"
done
chmod 600 "$temporary"
# A hard link atomically refuses to replace an existing .env.
ln "$temporary" .env
echo 'Created .env with five independent secrets. Fill in the credentials at the top, then run: sh manage deploy'
