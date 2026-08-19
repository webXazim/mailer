# CrescentSphere Mailer Backend

Rust services for the standalone transactional developer email platform.
Hosted mailbox protocols and mailbox storage are intentionally out of scope.

## Services

- `cs-mail-api`: Axum HTTP API, health endpoints, and request tracing.
- `cs-mail-worker`: asynchronous delivery and event processing host.
- PostgreSQL: authoritative application state.
- NATS JetStream: durable jobs and events, never the source of truth.

## Local development

Requirements: Rust stable and Docker with Compose.

```bash
cp .env.example .env
./manage dev
```

The API listens on `http://localhost:8080`. Liveness is available at
`GET /healthz`. `GET /readyz` checks PostgreSQL and NATS with bounded timeouts.
Database migrations run automatically when the API starts.

Authentication endpoints are available under `/v1/auth`: signup, login, logout,
session lookup, and password reset request/completion. Sessions are opaque,
random tokens stored only as SHA-256 hashes in PostgreSQL and delivered through
HttpOnly, SameSite cookies. Production uses a `__Host-` cookie with `Secure`;
local development uses a non-secure cookie for `http://localhost`.

API key management is available under `/v1/api-keys`. Secrets are shown only
on creation or rotation, stored as hashes, scoped to a workspace, and can be
revoked or expired. The verifier is ready for the send API and checks both
workspace ownership and the required permission scope.

Domain onboarding is available under `/v1/domains`. In production,
`DOMAIN_PROVIDER=ses` is required. Adding a domain creates an SES identity,
configures a custom MAIL FROM subdomain, and returns the required DKIM, SPF,
MX, and DMARC records. Verification polls SES and checks DNS; only domains
with verified SES sending status and required DNS records become `verified`.

The developer submission endpoint is `POST /v1/emails`. It requires an API
key with `emails:send`, a matching test/production environment, and an
`Idempotency-Key` header. The API validates the verified sender domain and
suppressions, then atomically stores the email, recipients, idempotency result,
and transactional outbox event before returning `202 Accepted`.

The worker publishes outbox events to a durable `MAILER_DELIVERY` JetStream
stream and consumes them with explicit acknowledgements, bounded retries, and
a `MAILER_DLQ` stream. Test-environment jobs are simulated locally; production
jobs use SES. A provider timeout after SES may have accepted a message is
treated as ambiguous: the email is quarantined for manual review after the
processing claim becomes stale instead of being silently resent.

SES delivery, bounce, complaint, reject, rendering-failure, open, and click
events are consumed from SQS through an SNS subscription. The worker verifies
the SNS topic, certificate URL, X.509 certificate, and RSA signature before
normalizing the SES notification and forwarding it to
`POST /internal/v1/ses/events`. The endpoint requires `EVENT_INGEST_TOKEN` and
is a replay-safe processor boundary: events are deduplicated, recipient and
aggregate states are monotonic, permanent bounces and complaints create
suppressions, delivered usage is counted once, and webhook work is written to
the outbox in the same transaction. Invalid SQS messages are left for the
queue's redrive policy and dead-letter queue.

Customer webhook endpoints are managed under `/v1/webhooks`. Endpoint secrets
are displayed only on creation or rotation and are derived from the stable
`WEBHOOK_SIGNING_MASTER_KEY`; PostgreSQL stores only their hashes. Delivery
rows and attempt history remain authoritative in PostgreSQL, while the worker
dispatches due attempts through the `MAILER_WEBHOOKS` JetStream stream. Calls
require HTTPS, reject private and link-local destinations during connection
resolution, time out after ten seconds, retry transient responses up to eight
times, and move terminal failures into `webhook_dead_letters`. Endpoints are
disabled after twenty consecutive terminal delivery failures.

Webhook requests contain `webhook-id`, `webhook-timestamp`, and
`webhook-signature` headers. The signature is `v1,` followed by URL-safe base64
HMAC-SHA256 over `<webhook-id>.<webhook-timestamp>.<raw-body>` using the
endpoint secret. Consumers should reject stale timestamps before comparing the
signature in constant time.

Start only the backend services from the `mailer/` root:

```bash
docker compose up --build api worker postgres nats
```

With the React app in `frontend/` running on `http://localhost:5173`, keep
`VITE_API_URL=http://localhost:8080` so requests use the API's `/v1` prefix.

## Verification

```bash
./manage check
```

## Production Configuration

Use [`../.env.production.example`](../.env.production.example) as the production
checklist. Store the real `.env` outside Git with file mode `0600`. Build and
publish immutable frontend, API, and worker images in CI, then deploy with:

```bash
docker compose --env-file .env \
  -f docker-compose.yml \
  -f docker-compose.production.yml \
  pull
docker compose --env-file .env \
  -f docker-compose.yml \
  -f docker-compose.production.yml \
  up -d --no-build
```

Only the API is bound to VPS loopback. Put Caddy, Nginx, or Traefik in front of
`127.0.0.1:8080` for `api.mailer.crescentsphere.com`. PostgreSQL and NATS have
no production host ports.

### AWS

Use `me-south-1` for SES, SNS, and SQS. On a VPS, create a dedicated IAM user
for this deployment; never use the AWS root account. Its policy should allow
only the SES identity/send operations used by the API and worker, plus
`sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:ChangeMessageVisibility`, and
`sqs:GetQueueAttributes` for the single event queue.

Create one SNS topic and one standard SQS queue with a redrive policy to a
separate DLQ. Subscribe the queue to the topic with raw message delivery
disabled, permit only that topic to call `sqs:SendMessage`, and configure the
SES configuration set to publish delivery, bounce, complaint, reject,
rendering-failure, open, and click events to the topic. Enter the final queue
URL and exact topic ARN as `SES_EVENTS_QUEUE_URL` and `SES_EVENTS_TOPIC_ARN`.

Provide the `API_AWS_*` and `WORKER_AWS_*` credentials only through the VPS
secret environment. Use separate IAM users: the API identity manages SES
domains, while the worker identity sends through SES and consumes the event
queue. If this later runs on AWS compute, use separate IAM roles and leave
static access-key variables empty.

### Cloudflare R2

Create a private bucket and an R2 API token restricted to Object Read and
Write for that bucket. Set the endpoint, bucket, access key, and secret in the
object-storage variables. Production requires object storage: the API writes
immutable message-content objects before making delivery work visible, and the
worker verifies each object's SHA-256 checksum before sending. Local
development can keep `OBJECT_STORAGE_PROVIDER=disabled` and store content in
PostgreSQL.

The send API accepts up to 10 base64 attachments, limited to 10 MB each and
20 MB decoded in total so the encoded MIME message remains within SES limits.
Attachments, inline content, HTML, and text are stored together in one
immutable message object. The worker generates multipart MIME and uses SES raw
content delivery. Terminal message objects are retained for
`EMAIL_CONTENT_RETENTION_DAYS`, then deleted only while PostgreSQL still marks
the email completed.

### Secrets

Generate the event token, webhook master key, and database password
independently. For example:

```bash
openssl rand -base64 48
```

Changing `WEBHOOK_SIGNING_MASTER_KEY` invalidates every existing customer
webhook secret. Back it up securely and rotate individual endpoints through
the API instead of replacing the master key during ordinary deployments.

### Abuse Limits

Email admission is serialized per workspace and enforced transactionally in
PostgreSQL. Defaults are configured with `API_KEY_RATE_LIMIT_PER_MINUTE`,
`CLIENT_IP_RATE_LIMIT_PER_MINUTE`, `WORKSPACE_MONTHLY_EMAIL_LIMIT`, and
`WORKSPACE_CONCURRENT_EMAIL_LIMIT`. Paid-plan overrides can be written to
`workspace_limits` without restarting services. The production reverse proxy
must connect over loopback and set `X-Real-IP`; forwarded IP headers are
ignored for non-loopback peers. Old minute buckets are removed hourly.

### Public-launch security gates

The API applies a 36 MB request body cap, a 30-second request deadline, strict
security response headers, exact-origin CORS, opaque HttpOnly session cookies,
and PostgreSQL-backed per-IP/API-key/workspace admission limits. Login attempts
are bucketed by both source IP and normalized email and return a generic failure
for unknown users or incorrect passwords; password-reset requests are always
accepted without revealing whether an account exists.

Keep `/internal/v1/ses/events` private to the worker/VPS network in the reverse
proxy. It still requires `EVENT_INGEST_TOKEN`, but must not be internet-facing.
Terminate TLS at the reverse proxy, enable HSTS there, and set only
`X-Real-IP` from that trusted loopback proxy. Never set `TRUST_PROXY_HEADERS=true`
when the API is directly exposed to the public network. Keep PostgreSQL, NATS,
and R2 private, rotate credentials independently, and rehearse database and
JetStream restoration before accepting customer traffic.

### VPS deployment and recovery

Install Docker Compose, Caddy, `age`, `rclone`, and `curl` on the VPS. Copy
`backend/deploy/Caddyfile` to Caddy's configuration directory and proxy only
`api.mailer.crescentsphere.com`; the Caddy rule deliberately returns `404` for
all `/internal/*` paths. Copy the real environment file to
`/opt/crescentsphere-mailer/.env` with mode `0600`, install the systemd units
and timers under `backend/deploy/systemd/`, then enable the backup and health timers.

The backup job creates an encrypted PostgreSQL custom dump, uploads it to the
configured immutable rclone remote, and prunes only local files older than the
configured retention period. Before launch and after every migration, run
`./manage restore-rehearsal` against its fixed disposable restore database.
Keep the age private identity offline from routine backup jobs; the backup job
needs only the public recipient.

The production Compose overlay binds the API to loopback and leaves PostgreSQL
and NATS without host ports. NATS uses unique authenticated credentials.
Application containers run as an unprivileged user with dropped capabilities,
no-new-privileges, read-only root filesystems, bounded PIDs, and bounded logs.

After entering production values, run `./manage preflight`. It requires the
environment file to be mode `0600`, rejects known placeholder/development
values, checks both public DNS names, validates the merged Compose model, and
validates the Caddy configuration without starting services. AWS policy
starting points are under `backend/deploy/aws/`; replace the account and configuration
set placeholders, then review the effective policy in AWS before creating
access keys.
