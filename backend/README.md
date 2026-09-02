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
sh manage dev
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
revoked or expired. The verifier enforces workspace ownership, permission scope, and the email environment. Rotation is transactional; revocation errors never return success.

Domain onboarding is available under `/v1/domains`. In production,
`DOMAIN_PROVIDER=ses` is required. Adding a domain creates an SES identity,
configures a custom MAIL FROM subdomain, and returns the required DKIM, SPF,
MX, DMARC, and unique ownership TXT records. Existing provider identities are reconciled after interrupted provisioning; disabling a Mailer domain preserves the SES identity so other applications are not disrupted. Verification polls SES and checks DNS; only domains
with verified SES sending status and required DNS records become `verified`.

The developer submission endpoint is `POST /v1/emails`. It requires an API
key with `emails:send` (or an owner/admin console session), a matching test/production environment, and an
`Idempotency-Key` header. The API validates the verified sender domain and
suppressions, then atomically stores the email, recipients, idempotency result,
and transactional outbox event before returning `202 Accepted`.

The worker publishes outbox events to a durable `MAILER_DELIVERY` JetStream
stream and consumes them with explicit acknowledgements, bounded retries, and
a `MAILER_DLQ` stream. Test-environment jobs are simulated locally; production
jobs use SES. A provider timeout after SES may have accepted a message is
treated as ambiguous: the email is quarantined for manual review after the
provider result is uncertain instead of being silently resent. Typed throttling errors retry with backoff; automatic SDK retries are disabled for sending. Shutdown drains bounded in-flight work; maintenance reconciles stale claims and expired queues.

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
sh manage check
```

## Production Configuration

Use [`../.env.production.example`](../.env.production.example). All credentials
are grouped first; generated local secrets stay stable between deployments.
See the [root VPS guide](../README.md#vps-deployment-testing-then-production) for
initial setup and the Cloudflare route. From the repository root:

```bash
sh manage deploy
```

This validates configuration, builds the API/worker/frontend on the VPS and
starts the standalone production Compose stack. Production no longer requires
CI-published images, Caddy or a separate API hostname. Do not merge the production
file with the development Compose file. The API base is
`https://mailer.crescentsphere.com/api`.

### AWS

Use `ap-southeast-1` for SES, SNS, and SQS. On a VPS, create a dedicated IAM user
for this deployment; never use the AWS root account. Its policy should allow
only the SES identity/send operations used by the API and worker, plus
`sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:ChangeMessageVisibility`, and
`sqs:GetQueueAttributes` for the single event queue.

Create one SNS topic and one standard SQS queue with a redrive policy to a
separate DLQ. Subscribe the queue to the topic with raw message delivery
disabled, permit only that topic to call `sqs:SendMessage`, and configure the
SES configuration set to publish delivery, bounce, complaint, reject,
rendering-failure, open, and click events to the topic. Enter the final queue
URL and exact topic ARN as `SES_EVENTS_QUEUE_URL` and `SES_EVENTS_TOPIC_ARN`. Set `SES_CONFIGURATION_SET` to this configuration-set name and ensure the worker IAM policy includes its ARN. Set `ACCOUNT_EMAIL_FROM` to an SES-verified sender for password-reset emails.

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
connects over loopback in the API network namespace and sets `X-Real-IP`; forwarded IP headers are
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

Follow the root VPS guide for deployment. `backend/deploy/Caddyfile` is a legacy
reference and is not used by the Cloudflare deployment. Production Compose uses
private networks, authenticated NATS, bounded logs/PIDs/resources, and unprivileged
read-only application containers. `sh manage preflight` checks credentials and
Compose without displaying secrets; it does not verify live provider permissions.

For offsite backups, install Bash, `age`, `rclone`, and `curl` on the VPS. Configure the
backup/alert values near the top of `.env` (mode `0600`). Install the units and
timers under `backend/deploy/systemd/` after reviewing their paths, then enable
the backup and health timers. The default checkout is `/opt/crescentsphere-mailer`.

The backup job creates an encrypted PostgreSQL custom dump, uploads it to the
configured immutable rclone remote, and prunes only local files older than the
retention period. Before launch and after every migration, run
`sh manage restore-rehearsal` against its fixed disposable database. Install a
compatible PostgreSQL client for its host-side `pg_restore --list` check. Keep
the age private identity offline from routine backup jobs; only restore needs it.
Never point the rehearsal at the live database or use it as a live rollback.

PostgreSQL backups do not cover NATS JetStream or R2 objects. Establish their
retention/recovery strategy and rehearse full recovery before accepting customers.
Changing the PostgreSQL password in `.env` alone does not change an existing
volume's database password. Coordinate database credential changes explicitly.
Changing the webhook master key invalidates existing customer webhook secrets.

AWS policy starting points are under `backend/deploy/aws/`; replace account and
configuration-set placeholders and review the effective permissions in AWS.


## Public beta API contract

- `POST /v1/auth/signup` accepts `email`, `password`, `first_name`, `last_name`, and
  `turnstile_token`. Production validates Turnstile server-side and queues a one-time
  email-verification link. The workspace name is created automatically.
- New workspaces can create test keys and simulate delivery. Production API keys and
  production submissions require operator approval through `sh manage approve-workspace`.
- `GET /v1/emails?limit=25&offset=0&environment=test` lists messages. Keys only see
  their environment; a console session can select either. `GET /v1/emails/{id}` returns
  status, recipients, up to 100 latest events, metadata, and retained body content.
- `POST /v1/emails` supports `from`, `to`, `cc`, `bcc`, `subject`, `text`, `html`,
  `reply_to`, `metadata`, `attachments`, and optional `environment` (inferred from the key).
  The total To/CC/BCC limit is 50. Nonempty `headers` or `tags` are explicitly rejected.
  Idempotency is scoped to workspace **and environment**; retry identical requests
  with the same key. Keys are retained without automatic expiry in this release.
- Test sends may use `sender@sandbox.mailer.invalid`. They generate persisted delivery
  events and webhooks without calling SES. Use `bounce@simulator.mailer.invalid` or
  `complaint@simulator.mailer.invalid` to test suppression. Suppressions are workspace-wide.
- `GET/POST /v1/suppressions` and `DELETE /v1/suppressions/{id}` require owner/admin or
  `suppressions:manage`; POST accepts `{ "address": "recipient@example.com" }`.
- Domain management accepts `domains:read`/`domains:write`; webhook management accepts
  `webhooks:manage`; workspace details accept `workspace:read`. Control-plane permissions
  manage shared workspace resources regardless of a key's sending environment.
- Creating a webhook requires `{ "url": "https://hooks.example.com/events",
  "environment": "test", "subscriptions": ["email.delivery"] }`. Its environment is
  fixed; recreate to change it. Literal IP addresses, local names, and private DNS
  destinations are blocked. Existing endpoints migrate to `production`.
- Webhook payload `{id,type,createdAt,data}` includes `data.emailId`, `data.environment`,
  and `data.metadata`. `type` matches subscriptions (`email.delivery`, `email.bounce`, etc.).
  The signature uses the **complete whsec_ secret as UTF-8**, not a decoded key;
  output is unpadded base64url. Check timestamps and deduplicate webhook-id. Retries
  may duplicate delivery; email API idempotency does not make webhook handling exactly once.

The monthly limit counts accepted message submissions, including test messages, not
recipients/provider billing units. Concurrency/rate limits are additional safeguards.
SES account quotas and reputation need independent monitoring. Templates, custom headers,
tags, billing, MFA, and team administration are deferred.

## Recovery and retention

Password-reset requests enqueue expiring messages in `account_emails`. The worker sends
them from `ACCOUNT_EMAIL_FROM`, clears the raw link on success/failure/expiry, and only
retries definite throttling responses. Users can request another link after failure.
Reset completion invalidates outstanding reset tokens and all existing sessions.

Failed or uncertain developer sends are visible in email details with `lastError` and
in `delivery_dead_letters`. An operator must reconcile uncertain results with SES
before sending a replacement using a **new** idempotency key. There is deliberately
no blind resend button for ambiguous provider acceptance.

Content retention also covers `sent` messages whose final provider event never arrived.
Configure an R2/S3 lifecycle safety net for orphaned `workspaces/` objects at a duration
longer than `EMAIL_CONTENT_RETENTION_DAYS` plus the seven-day queue window. The lifecycle
rule is an operator setup step; the application cannot enumerate uncertain orphan objects.

## Repeatable integration checks

From the repository root, build the isolated test images named in
`backend/deploy/tests/integration.py`, then run that script with Python 3. It creates and
removes only a unique test stack, uses fake credentials, and never starts cloudflared.
Production requests in the suite are not passed to SES. The optional `--keep` flag is
for local browser QA and requires explicit cleanup of the recorded test project afterward.
