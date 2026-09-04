#!/bin/sh
# Run inside a disposable Linux container with /source containing manage,
# .env.production.example and backend/deploy. Never reads the real .env.
set -eu
mkdir -p /test/backend /test/bin
cp -R /source/backend/deploy /test/backend/
cp /source/manage /source/.env.production.example /test/
cd /test
for file in manage backend/deploy/*.sh; do sh -n "$file"; done
sh manage production-init
test "$(stat -c '%a' .env)" = 600
before=$(sha256sum .env)
if sh manage production-init >/dev/null 2>&1; then
    echo 'Initialization overwrote .env' >&2; exit 1
fi
test "$before" = "$(sha256sum .env)"
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
chmod 644 .env
if sh manage preflight >/dev/null 2>&1; then exit 1; fi
chmod 600 .env
cp .env valid.env
sed -i 's/^DELIVERY_PROVIDER=ses$/DELIVERY_PROVIDER=smtp/' .env
sed -i 's/^SMTP_HOST=$/SMTP_HOST=smtp.example.test/' .env
sed -i 's/^SMTP_USERNAME=$/SMTP_USERNAME=mailer-worker/' .env
sed -i 's/^SMTP_PASSWORD=$/SMTP_PASSWORD=test-smtp-password/' .env
sed -i 's/^WORKER_AWS_ACCESS_KEY_ID=.*/WORKER_AWS_ACCESS_KEY_ID=/' .env
sed -i 's/^WORKER_AWS_SECRET_ACCESS_KEY=.*/WORKER_AWS_SECRET_ACCESS_KEY=/' .env
sed -i 's/^SES_EVENTS_QUEUE_URL=.*/SES_EVENTS_QUEUE_URL=/' .env
sed -i 's/^SES_EVENTS_TOPIC_ARN=.*/SES_EVENTS_TOPIC_ARN=/' .env
sed -i 's/^SES_CONFIGURATION_SET=.*/SES_CONFIGURATION_SET=/' .env
sh manage preflight
sed -i 's/^SMTP_SECURITY=implicit_tls$/SMTP_SECURITY=plaintext/' .env
if sh manage preflight >/dev/null 2>&1; then exit 1; fi
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
