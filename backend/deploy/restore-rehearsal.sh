#!/bin/sh
set -eu

umask 077
: "${BACKUP_RCLONE_REMOTE:?BACKUP_RCLONE_REMOTE is required}"
: "${BACKUP_AGE_IDENTITY_FILE:?BACKUP_AGE_IDENTITY_FILE is required}"
: "${POSTGRES_USER:?POSTGRES_USER is required}"

temporary_dir=$(mktemp -d)
cleanup() {
    docker compose --env-file .env --project-name crescentsphere-mailer -f docker-compose.production.yml \
        exec -T postgres dropdb --if-exists --username "$POSTGRES_USER" mailer_restore_rehearsal >/dev/null 2>&1 || true
    rm -rf "$temporary_dir"
}
trap cleanup EXIT INT TERM

latest=$(rclone lsf "$BACKUP_RCLONE_REMOTE" --files-only --include 'postgres-*.dump.age' | sort | tail -n 1)
test -n "$latest"
rclone copyto "$BACKUP_RCLONE_REMOTE/$latest" "$temporary_dir/backup.dump.age"
age --decrypt --identity "$BACKUP_AGE_IDENTITY_FILE" \
    --output "$temporary_dir/backup.dump" "$temporary_dir/backup.dump.age"
pg_restore --list "$temporary_dir/backup.dump" >/dev/null

docker compose --env-file .env --project-name crescentsphere-mailer -f docker-compose.production.yml \
    exec -T postgres createdb --username "$POSTGRES_USER" mailer_restore_rehearsal
docker compose --env-file .env --project-name crescentsphere-mailer -f docker-compose.production.yml \
    exec -T postgres pg_restore --exit-on-error --no-owner --no-acl \
    --username "$POSTGRES_USER" --dbname mailer_restore_rehearsal < "$temporary_dir/backup.dump"
docker compose --env-file .env --project-name crescentsphere-mailer -f docker-compose.production.yml \
    exec -T postgres psql --username "$POSTGRES_USER" --dbname mailer_restore_rehearsal \
    --tuples-only --command "SELECT count(*) FROM _sqlx_migrations;" | grep -Eq '[1-9][0-9]*'
