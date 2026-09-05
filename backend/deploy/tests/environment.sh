#!/bin/sh
# Run inside a disposable Linux container with /source containing manage,
# .env.production.example and backend/deploy. Never reads the real .env.
set -eu
mkdir -p /test/backend /test/bin
cp -R /source/backend/deploy /test/backend/
cp /source/manage /source/.env.production.example /source/.env.storage.example /source/docker-compose.storage.yml /test/
cd /test
for file in manage backend/deploy/*.sh; do sh -n "$file"; done
sh manage production-init
test "$(stat -c '%a' .env)" = 600
before=$(sha256sum .env)
if sh manage production-init >/dev/null 2>&1; then
    echo 'Initialization overwrote .env' >&2; exit 1
fi
test "$before" = "$(sha256sum .env)"
sed -i '/^DELIVERY_PROVIDER=/d' .env
sh manage production-env-upgrade
test "$(grep -c '^DELIVERY_PROVIDER=ses$' .env)" = 1
test "$(stat -c '%a' .env)" = 600
set -a
. ./.env
set +a
test "${#POSTGRES_PASSWORD}" = 64
test "${#NATS_PASSWORD}" = 64
test "$POSTGRES_PASSWORD" != "$NATS_PASSWORD"
test "$EVENT_INGEST_TOKEN" != "$WEBHOOK_SIGNING_MASTER_KEY"
if sh manage preflight >failure.log 2>&1; then exit 1; fi
grep -q 'CLOUDFLARE_TUNNEL_TOKEN is required' failure.log

for name in CLOUDFLARE_TUNNEL_TOKEN API_AWS_ACCESS_KEY_ID API_AWS_SECRET_ACCESS_KEY WORKER_AWS_ACCESS_KEY_ID WORKER_AWS_SECRET_ACCESS_KEY SES_EVENTS_QUEUE_URL SES_EVENTS_TOPIC_ARN SES_CONFIGURATION_SET OBJECT_STORAGE_ENDPOINT OBJECT_STORAGE_BUCKET OBJECT_STORAGE_ACCESS_KEY_ID OBJECT_STORAGE_SECRET_ACCESS_KEY; do
    sed -i "s/^$name=\$/$name=test-credential/" .env
done
sed -i 's/^TURNSTILE_SITE_KEY=$/TURNSTILE_SITE_KEY=test-site-key/' .env
sed -i 's/^TURNSTILE_SECRET_KEY=$/TURNSTILE_SECRET_KEY=test-secret-key-that-is-long-enough/' .env
sed -i 's/^ACCOUNT_EMAIL_API_KEY=$/ACCOUNT_EMAIL_API_KEY=cs_live_test-account-email-key/' .env
# The shim verifies command sequencing without contacting Docker or providers.
cat >bin/docker <<'EOF'
#!/bin/sh
echo "$*" >>/test/docker-calls.log
case " $* " in
    *' build '*) test "${FAIL_BUILD:-0}" = 0 ;;
    *' port '*) echo '127.0.0.1:49152' ;;
esac
EOF
chmod +x bin/docker
export PATH="/test/bin:$PATH"
sh manage preflight
sh manage storage-init
test "$(stat -c '%a' .env.storage)" = 600
test "$(stat -c '%a' .storage/garage.toml)" = 600
grep -Eq '^rpc_secret = "[a-f0-9]{64}"$' .storage/garage.toml
sh manage storage-preflight
: >docker-calls.log
sh manage storage-up
grep -q 'pull garage' docker-calls.log
grep -q 'up -d --wait --wait-timeout 120 garage' docker-calls.log
chmod 644 .env
if sh manage preflight >/dev/null 2>&1; then exit 1; fi
chmod 600 .env
cp .env valid.env
sed -i 's/^DELIVERY_PROVIDER=ses$/DELIVERY_PROVIDER=smtp/' .env
sed -i 's/^SMTP_HOST=$/SMTP_HOST=smtp.example.test/' .env
sed -i 's/^SMTP_USERNAME=$/SMTP_USERNAME=mailer-worker/' .env
sed -i 's/^SMTP_PASSWORD=$/SMTP_PASSWORD=test-smtp-password/' .env
sed -i 's/^STALWART_WEBHOOK_TOKEN=$/STALWART_WEBHOOK_TOKEN=0123456789abcdef0123456789abcdef/' .env
sed -i 's/^STALWART_WEBHOOK_SIGNING_KEY=$/STALWART_WEBHOOK_SIGNING_KEY=fedcba9876543210fedcba9876543210/' .env
sed -i 's/^WORKER_AWS_ACCESS_KEY_ID=.*/WORKER_AWS_ACCESS_KEY_ID=/' .env
sed -i 's/^WORKER_AWS_SECRET_ACCESS_KEY=.*/WORKER_AWS_SECRET_ACCESS_KEY=/' .env
sed -i 's/^SES_EVENTS_QUEUE_URL=.*/SES_EVENTS_QUEUE_URL=/' .env
sed -i 's/^SES_EVENTS_TOPIC_ARN=.*/SES_EVENTS_TOPIC_ARN=/' .env
sed -i 's/^SES_CONFIGURATION_SET=.*/SES_CONFIGURATION_SET=/' .env
sh manage preflight
sed -i 's/^SMTP_SECURITY=implicit_tls$/SMTP_SECURITY=plaintext/' .env
if sh manage preflight >/dev/null 2>&1; then exit 1; fi
cp valid.env .env
sed -i 's/^DOMAIN_PROVIDER=ses$/DOMAIN_PROVIDER=stalwart/' .env
sed -i 's|^STALWART_API_URL=$|STALWART_API_URL=http://stalwart:8080|' .env
sed -i 's/^STALWART_API_TOKEN=$/STALWART_API_TOKEN=0123456789abcdef0123456789abcdef/' .env
sh manage preflight
cp valid.env .env
sed -i 's/^POSTGRES_PASSWORD=.*/POSTGRES_PASSWORD=unsafe-password-with-special-char@/' .env
if sh manage preflight >/dev/null 2>&1; then exit 1; fi
cp valid.env .env

: >docker-calls.log
if FAIL_BUILD=1 sh manage deploy >/dev/null 2>&1; then exit 1; fi
if grep -q ' up ' docker-calls.log; then
    echo 'Deployment started containers after a failed build' >&2; exit 1
fi
: >docker-calls.log
sh manage deploy
grep -q 'build api worker frontend' docker-calls.log
grep -q 'up -d --no-build --wait --wait-timeout 180' docker-calls.log
grep -q 'port api 8081' docker-calls.log
if grep -q 'down\|prune\|--volumes' docker-calls.log; then exit 1; fi
sh manage smtp-pause
sh manage smtp-resume
sh manage smtp-cap 25
sh manage ses-rollback enable
sh manage route-workspace 11111111-1111-4111-8111-111111111111 smtp
sh manage route-workspace 11111111-1111-4111-8111-111111111111 default
sh manage delivery-routing-status
sh manage delivery-report 7
sh manage pause-workspace 11111111-1111-4111-8111-111111111111
sh manage resume-workspace 11111111-1111-4111-8111-111111111111
sh manage security-events 7
if sh manage smtp-cap invalid >/dev/null 2>&1; then exit 1; fi
if sh manage pause-workspace invalid >/dev/null 2>&1; then exit 1; fi
grep -q 'delivery_operator_controls' docker-calls.log
grep -q 'workspace_delivery_routes' docker-calls.log
grep -q 'security.workspace_paused' docker-calls.log
grep -q 'security.workspace_resumed' docker-calls.log
echo 'Environment initialization, validation and deployment sequencing passed.'

# A failed database dump must not become an apparently valid encrypted backup.
cat >bin/docker <<'EOF'
#!/bin/sh
exit 42
EOF
cat >bin/age <<'EOF'
#!/bin/sh
cat >"$4"
EOF
cat >bin/rclone <<'EOF'
#!/bin/sh
touch /test/unexpected-upload
EOF
chmod +x bin/docker bin/age bin/rclone
if BACKUP_DIR=/var/backups/mailer-test BACKUP_AGE_RECIPIENT=test \
    BACKUP_RCLONE_REMOTE=test:backup bash backend/deploy/backup.sh; then
    echo 'Backup concealed a pg_dump failure' >&2; exit 1
fi
test ! -e /test/unexpected-upload
test -z "$(find /var/backups/mailer-test -type f -print)"
echo 'Backup pipeline failure propagation passed.'

# A successful object backup must use a temporary credential file, never expose
# its secret in command arguments, and copy to the immutable offsite prefix.
cat >bin/docker <<'EOF'
#!/bin/sh
printf 'database dump'
EOF
cat >bin/rclone <<'EOF'
#!/bin/sh
echo "$*" >>/test/rclone-calls.log
if [ "${1:-}" = --config ]; then
    test -f "$2"
    grep -q '^secret_access_key = object-secret$' "$2"
    touch /test/object-copy-ran
fi
EOF
chmod +x bin/docker bin/rclone
: >rclone-calls.log
BACKUP_DIR=/var/backups/mailer-success BACKUP_AGE_RECIPIENT=test \
BACKUP_RCLONE_REMOTE=test:backup BACKUP_OBJECT_STORAGE=true \
OBJECT_STORAGE_ENDPOINT=http://garage:3900 OBJECT_STORAGE_BACKUP_ENDPOINT=http://127.0.0.1:3900 \
OBJECT_STORAGE_BUCKET=mailer OBJECT_STORAGE_ACCESS_KEY_ID=object-key \
OBJECT_STORAGE_SECRET_ACCESS_KEY=object-secret OBJECT_STORAGE_REGION=garage \
bash backend/deploy/backup.sh
test -e /test/object-copy-ran
grep -q 'object-storage --immutable' rclone-calls.log
if grep -q 'object-secret' rclone-calls.log; then
    echo 'Object-storage secret leaked into rclone arguments' >&2; exit 1
fi
echo 'Immutable object backup and credential isolation passed.'
