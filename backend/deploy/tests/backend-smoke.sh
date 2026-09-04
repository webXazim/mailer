#!/bin/sh
# Real database/NATS startup regression. Never start cloudflared or the worker.
set -eu
cd "$(dirname "$0")/../../.."
project="mailer-smoke-$$"
temporary=$(mktemp -d)
created_mail_network=
compose() {
    docker compose --project-name "$project" --env-file "$temporary/test.env" \
        -f docker-compose.production.yml "$@"
}
cleanup() {
    # This unique test project owns only disposable data created by this script.
    compose down --volumes >/dev/null 2>&1 || true
    if [ "$created_mail_network" = 1 ]; then
        docker network rm crescentsphere-mail-transport >/dev/null 2>&1 || true
    fi
    rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM
cp .env.production.example "$temporary/test.env"
for name in POSTGRES_PASSWORD NATS_PASSWORD EVENT_INGEST_TOKEN WEBHOOK_SIGNING_MASTER_KEY STALWART_WEBHOOK_TOKEN STALWART_WEBHOOK_SIGNING_KEY; do
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
if ! docker network inspect crescentsphere-mail-transport >/dev/null 2>&1; then
    docker network create crescentsphere-mail-transport >/dev/null
    created_mail_network=1
fi
COMPOSE_PARALLEL_LIMIT=1 compose build api frontend
if ! compose up -d --no-build --wait --wait-timeout 180 api frontend; then
    compose logs --tail 30 api postgres nats
    exit 1
fi
compose exec -T api curl --fail --silent http://127.0.0.1:8081/api/readyz
status=$(compose exec -T api curl --silent --output /dev/null --write-out '%{http_code}' \
    http://127.0.0.1:8081/api/internal/v1/ses/events || true)
test "$status" = 404
status=$(compose exec -T api curl --silent --output /dev/null --write-out '%{http_code}' \
    http://127.0.0.1:8081/api/internal/v1/stalwart/events || true)
test "$status" = 404
compose exec -T postgres psql --username mailer --dbname mailer --tuples-only \
    --command 'SELECT count(*) FROM _sqlx_migrations;' | grep -Eq '[1-9][0-9]*'

compose exec -T postgres psql --username mailer --dbname mailer <<'SQL'
INSERT INTO users (id,email,password_hash,display_name,email_verified_at)
VALUES ('11111111-1111-4111-8111-111111111111','events@example.test','unused','Events',now());
INSERT INTO workspaces (id,name,slug,created_by)
VALUES ('22222222-2222-4222-8222-222222222222','Events','events','11111111-1111-4111-8111-111111111111');
INSERT INTO emails (id,workspace_id,environment,sender,subject,status,delivery_provider,sent_at)
VALUES ('33333333-3333-4333-8333-333333333333','22222222-2222-4222-8222-222222222222','production','sender@example.test','Webhook test','sent','smtp',now());
INSERT INTO email_recipients (email_id,address,recipient_type,status) VALUES
('33333333-3333-4333-8333-333333333333','one@example.test','to','sent'),
('33333333-3333-4333-8333-333333333333','two@example.test','to','sent');
INSERT INTO delivery_provider_attempts (id,email_id,provider,attempt_number,status,provider_message_id,completed_at)
VALUES ('44444444-4444-4444-8444-444444444444','33333333-3333-4333-8333-333333333333','smtp',1,'submitted','33333333-3333-4333-8333-333333333333.44444444-4444-4444-8444-444444444444@smtp.example.test',now());
SQL

cat >"$temporary/event-one.json" <<'JSON'
{"events":[{"id":"stalwart-event-1","createdAt":"2026-09-04T00:00:00Z","type":"delivery.delivered","data":{"messageId":"33333333-3333-4333-8333-333333333333.44444444-4444-4444-8444-444444444444@smtp.example.test","to":"one@example.test"}}]}
JSON
signature=$(openssl dgst -sha256 -hmac "$STALWART_WEBHOOK_SIGNING_KEY" -binary "$temporary/event-one.json" | openssl base64 -A)
compose exec -T api curl --fail --silent \
    -H "Authorization: Bearer $STALWART_WEBHOOK_TOKEN" -H "X-Signature: $signature" \
    -H 'Content-Type: application/json' --data-binary @- \
    http://127.0.0.1:8080/internal/v1/stalwart/events <"$temporary/event-one.json" >/dev/null
test "$(compose exec -T postgres psql -At --username mailer --dbname mailer --command "SELECT status FROM emails WHERE id='33333333-3333-4333-8333-333333333333'")" = sent

cat >"$temporary/event-two.json" <<'JSON'
{"events":[{"id":"stalwart-event-2","createdAt":"2026-09-04T00:00:01Z","type":"delivery.delivered","data":{"messageId":"<33333333-3333-4333-8333-333333333333.44444444-4444-4444-8444-444444444444@smtp.example.test>","to":["two@example.test"]}}]}
JSON
signature=$(openssl dgst -sha256 -hmac "$STALWART_WEBHOOK_SIGNING_KEY" -binary "$temporary/event-two.json" | openssl base64 -A)
compose exec -T api curl --fail --silent \
    -H "Authorization: Bearer $STALWART_WEBHOOK_TOKEN" -H "X-Signature: $signature" \
    -H 'Content-Type: application/json' --data-binary @- \
    http://127.0.0.1:8080/internal/v1/stalwart/events <"$temporary/event-two.json" >/dev/null
compose exec -T api curl --fail --silent \
    -H "Authorization: Bearer $STALWART_WEBHOOK_TOKEN" -H "X-Signature: $signature" \
    -H 'Content-Type: application/json' --data-binary @- \
    http://127.0.0.1:8080/internal/v1/stalwart/events <"$temporary/event-two.json" >/dev/null
test "$(compose exec -T postgres psql -At --username mailer --dbname mailer --command "SELECT status FROM emails WHERE id='33333333-3333-4333-8333-333333333333'")" = delivered
test "$(compose exec -T postgres psql -At --username mailer --dbname mailer --command "SELECT count(*) FROM delivery_events WHERE email_id='33333333-3333-4333-8333-333333333333'")" = 2
test "$(compose exec -T postgres psql -At --username mailer --dbname mailer --command "SELECT emails_delivered FROM usage_counters WHERE workspace_id='22222222-2222-4222-8222-222222222222'")" = 1

echo 'Real API, authenticated NATS, migrations, signed Stalwart events, replay safety, and Nginx isolation passed.'
