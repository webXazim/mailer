#!/bin/sh
set -eu

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

cleanup() { rm -f "$temporary"; }
trap cleanup EXIT INT TERM

docker compose --env-file .env -f docker-compose.yml -f docker-compose.production.yml \
    exec -T postgres pg_dump --format=custom --no-owner --no-acl \
    --username "$POSTGRES_USER" "$POSTGRES_DB" \
    | age --recipient "$BACKUP_AGE_RECIPIENT" --output "$temporary"

test -s "$temporary"
mv "$temporary" "$archive"
rclone copyto "$archive" "$BACKUP_RCLONE_REMOTE/$(basename "$archive")" --immutable
find "$BACKUP_DIR" -type f -name 'postgres-*.dump.age' -mtime "+$BACKUP_RETENTION_DAYS" -delete
trap - EXIT INT TERM
