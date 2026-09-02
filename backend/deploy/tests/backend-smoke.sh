#!/bin/sh
# Real database/NATS startup regression. Never start cloudflared or the worker.
set -eu
cd "$(dirname "$0")/../../.."
project="mailer-smoke-$$"
temporary=$(mktemp -d)
compose() {
    docker compose --project-name "$project" --env-file "$temporary/test.env" \
        -f docker-compose.production.yml "$@"
}
cleanup() {
    # This unique test project owns only disposable data created by this script.
    compose down --volumes >/dev/null 2>&1 || true
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM
cp .env.production.example "$temporary/test.env"
for name in POSTGRES_PASSWORD NATS_PASSWORD EVENT_INGEST_TOKEN WEBHOOK_SIGNING_MASTER_KEY; do
    value=$(openssl rand -hex 32)
    sed -i "s/^$name=\$/$name=$value/" "$temporary/test.env"
done
for name in CLOUDFLARE_TUNNEL_TOKEN TURNSTILE_SITE_KEY TURNSTILE_SECRET_KEY API_AWS_ACCESS_KEY_ID API_AWS_SECRET_ACCESS_KEY WORKER_AWS_ACCESS_KEY_ID WORKER_AWS_SECRET_ACCESS_KEY OBJECT_STORAGE_ACCESS_KEY_ID OBJECT_STORAGE_SECRET_ACCESS_KEY; do
    sed -i "s/^$name=\$/$name=unused-test-credential/" "$temporary/test.env"
done
cat >>"$temporary/test.env" <<'EOF'
SES_EVENTS_QUEUE_URL=https://sqs.ap-southeast-1.amazonaws.com/000000000000/unused
SES_EVENTS_TOPIC_ARN=arn:aws:sns:ap-southeast-1:000000000000:unused
OBJECT_STORAGE_ENDPOINT=http://127.0.0.1:9
OBJECT_STORAGE_BUCKET=unused
SES_CONFIGURATION_SET=unused
ACCOUNT_EMAIL_FROM=unused@example.com
IMAGE_TAG=deployment-smoke
EOF
# Override inherited shell values with these deliberately fake credentials.
set -a
. "$temporary/test.env"
set +a
COMPOSE_PARALLEL_LIMIT=1 compose build api frontend
if ! compose up -d --no-build --wait --wait-timeout 180 api frontend; then
    compose logs --tail 30 api postgres nats
    exit 1
fi
compose exec -T api curl --fail --silent http://127.0.0.1:8081/api/readyz
status=$(compose exec -T api curl --silent --output /dev/null --write-out '%{http_code}' \
    http://127.0.0.1:8081/api/internal/v1/ses/events)
test "$status" = 404
compose exec -T postgres psql --username mailer --dbname mailer --tuples-only \
    --command 'SELECT count(*) FROM _sqlx_migrations;' | grep -Eq '[1-9][0-9]*'
echo 'Real API, authenticated NATS, migrations and Nginx smoke test passed.'
