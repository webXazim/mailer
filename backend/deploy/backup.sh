#!/usr/bin/env bash
# A failed pg_dump must fail the pipeline, even if age encrypts an empty stream.
set -euo pipefail

umask 077
: "${BACKUP_DIR:?BACKUP_DIR is required}"
: "${BACKUP_AGE_RECIPIENT:?BACKUP_AGE_RECIPIENT is required}"
: "${BACKUP_RCLONE_REMOTE:?BACKUP_RCLONE_REMOTE is required}"
: "${BACKUP_RETENTION_DAYS:=14}"

case "$BACKUP_DIR" in
    /var/backups/*) ;;
    *) echo "BACKUP_DIR must be below /var/backups" >&2; exit 1 ;;
esac

mkdir -p "$BACKUP_DIR"
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
archive="$BACKUP_DIR/postgres-$timestamp.dump.age"
temporary="$archive.partial"
rclone_config=

cleanup() { rm -f "$temporary" ${rclone_config:+"$rclone_config"}; }
trap cleanup EXIT INT TERM

docker compose --env-file .env --project-name crescentsphere-mailer -f docker-compose.production.yml \
    exec -T postgres pg_dump --format=custom --no-owner --no-acl \
    --username "$POSTGRES_USER" "$POSTGRES_DB" \
    | age --recipient "$BACKUP_AGE_RECIPIENT" --output "$temporary"

test -s "$temporary"
mv "$temporary" "$archive"
rclone copyto "$archive" "$BACKUP_RCLONE_REMOTE/$(basename "$archive")" --immutable
if [ "${BACKUP_OBJECT_STORAGE:-false}" = true ]; then
    : "${OBJECT_STORAGE_ENDPOINT:?OBJECT_STORAGE_ENDPOINT is required for object backup}"
    : "${OBJECT_STORAGE_BUCKET:?OBJECT_STORAGE_BUCKET is required for object backup}"
    : "${OBJECT_STORAGE_ACCESS_KEY_ID:?OBJECT_STORAGE_ACCESS_KEY_ID is required for object backup}"
    : "${OBJECT_STORAGE_SECRET_ACCESS_KEY:?OBJECT_STORAGE_SECRET_ACCESS_KEY is required for object backup}"
    source_endpoint=${OBJECT_STORAGE_BACKUP_ENDPOINT:-$OBJECT_STORAGE_ENDPOINT}
    rclone_config=$(mktemp)
    chmod 600 "$rclone_config"
    cat >"$rclone_config" <<EOF
[mailer-source]
type = s3
provider = Other
access_key_id = $OBJECT_STORAGE_ACCESS_KEY_ID
secret_access_key = $OBJECT_STORAGE_SECRET_ACCESS_KEY
endpoint = $source_endpoint
region = ${OBJECT_STORAGE_REGION:-auto}
force_path_style = true
EOF
    # Mailer object keys are immutable UUID paths. --immutable prevents a
    # compromised source from replacing an already archived object.
    rclone --config "$rclone_config" copy \
        "mailer-source:$OBJECT_STORAGE_BUCKET" \
        "$BACKUP_RCLONE_REMOTE/object-storage" --immutable
fi
find "$BACKUP_DIR" -type f -name 'postgres-*.dump.age' -mtime "+$BACKUP_RETENTION_DAYS" -delete
trap - EXIT INT TERM
