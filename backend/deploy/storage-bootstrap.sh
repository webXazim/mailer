#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
set -a
. ./.env.storage
set +a
compose() {
    docker compose --project-name crescentsphere-storage --env-file .env.storage -f docker-compose.storage.yml "$@"
}
garage() { compose exec -T garage /garage "$@"; }

echo 'Checking Garage node and layout...'
status=$(garage status)
if printf '%s\n' "$status" | grep -Fq 'NO ROLE ASSIGNED'; then
    node_id=$(garage node id -q | tail -n 1 | cut -d@ -f1)
    test -n "$node_id"
    echo 'Assigning the first storage layout...'
    garage layout assign -z "$GARAGE_ZONE" -c "$GARAGE_CAPACITY" "$node_id"
    garage layout apply --version 1
fi
echo 'Checking for an existing bucket or application key...'
if garage key list | grep -Fq "$GARAGE_KEY_NAME"; then
    echo 'Application key already exists; refusing to rotate or duplicate credentials.' >&2
    exit 1
fi
echo 'Creating the bucket and application key...'
if ! garage bucket list | grep -Fq "$GARAGE_BUCKET"; then
    garage bucket create "$GARAGE_BUCKET"
fi
key_output=.storage/key-create.output
rm -f "$key_output"
umask 077
garage key create "$GARAGE_KEY_NAME" >"$key_output"
access_key=$(awk '/Key ID:/ {print $NF; exit}' "$key_output")
secret_key=$(awk '/Secret key:/ {print $NF; exit}' "$key_output")
test -n "$access_key" && test -n "$secret_key"
cat >.storage/mailer.env <<EOF
OBJECT_STORAGE_PROVIDER=s3
OBJECT_STORAGE_ENDPOINT=http://garage:3900
OBJECT_STORAGE_REGION=garage
OBJECT_STORAGE_BUCKET=$GARAGE_BUCKET
OBJECT_STORAGE_ACCESS_KEY_ID=$access_key
OBJECT_STORAGE_SECRET_ACCESS_KEY=$secret_key
EOF
chmod 600 .storage/mailer.env
rm -f "$key_output"
garage bucket allow --read --write "$GARAGE_BUCKET" --key "$GARAGE_KEY_NAME"
echo 'Garage layout, bucket, and least-privilege application key created.'
echo 'Credentials were written to .storage/mailer.env (mode 600); copy those six values into .env.'
