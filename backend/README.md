# CrescentSphere Mailer backend

Rust workspace for the Axum API and asynchronous delivery worker. Production
mail is submitted only to the independently operated Stalwart SMTP service.

## Services

- `cs-mail-api` owns authentication, API keys, domain onboarding, email
  admission, activity, suppressions, and customer webhooks.
- `cs-mail-worker` consumes JetStream jobs, builds MIME messages, submits them
  over authenticated TLS SMTP, processes lifecycle jobs, and dispatches
  webhooks.
- PostgreSQL is the durable source of truth. NATS JetStream is the work signal;
  the transactional outbox prevents accepted jobs from being lost.
- Email content and attachments use the configured S3-compatible object store.

## Independent mail transport

Configure these values together:

- `SMTP_HOST`, `SMTP_PORT`, `SMTP_SECURITY`, `SMTP_USERNAME`, `SMTP_PASSWORD`
- `SMTP_HELO_NAME` and `SMTP_TIMEOUT_SECONDS`
- `STALWART_API_URL` and `STALWART_API_TOKEN`
- `MTA_PUBLIC_HOST`, `MTA_PUBLIC_IPV4`, `MTA_RETURN_PATH_PREFIX`
- `STALWART_WEBHOOK_TOKEN` and `STALWART_WEBHOOK_SIGNING_KEY`

Use `implicit_tls` on port 465 or `starttls` on port 587. Plaintext SMTP is not
supported. The Stalwart management URL should be private; its API credential
needs only the domain and DKIM permissions described in
`STALWART_DOMAIN_PROVISIONING.md`.

Domain onboarding provisions a Stalwart domain and DKIM signature, then returns
the DNS records for the platform's own host and IPv4 address:

- DKIM TXT under `<selector>._domainkey.<sending-domain>`
- MX and SPF for `<return-path-prefix>.<sending-domain>`
- DMARC TXT under `_dmarc.<sending-domain>`
- CrescentSphere ownership TXT under `_mailer-verification.<sending-domain>`

The background verifier checks required DNS records before enabling production
sending. DKIM rotation keeps the previous signature until the replacement TXT
record verifies.

Stalwart sends signed event batches to `POST /internal/v1/stalwart/events`.
Requests require both the bearer token and the base64 HMAC-SHA256 signature of
the exact request body. Keep this route private; the production Nginx proxy
blocks `/internal` from public access.

## Delivery safety

Production admission checks workspace status, a verified sender domain,
suppression state, monthly usage, concurrency, SMTP pause state, and the daily
SMTP cap. Each real provider call creates an append-only
`delivery_provider_attempts` row before network I/O. Ambiguous transport results
are never retried automatically because the remote MTA may already have
accepted the message.

Test keys use `sender@sandbox.mailer.invalid` and never contact SMTP. Simulator
recipients include `bounce@simulator.mailer.invalid` and
`complaint@simulator.mailer.invalid`.

Operator controls:

```bash
sh manage smtp-pause
sh manage smtp-resume
sh manage smtp-cap 1000
sh manage delivery-routing-status
sh manage delivery-report 7
```

## Object storage

`OBJECT_STORAGE_PROVIDER` may be `disabled`, `r2`, or `s3`; both enabled modes
use the S3-compatible protocol. Configure the endpoint, bucket, region, access
key, secret, and timeout together. Production requires durable object storage.

## Verification

From the repository root:

```bash
sh manage backend-fmt
sh manage backend-lint
sh manage backend-test
sh manage compose-config
```

The isolated Docker regression suites are under `backend/deploy/tests`. They use
disposable Compose projects and never read the real `.env` file.

## Deployment

Use `sh manage production-init`, complete `.env`, start the independent Stalwart
stack, and run `sh manage deploy`. Production validation requires authenticated
NATS, HTTPS console origin, Turnstile, Stalwart/SMTP credentials, signed event
credentials, abuse limits, and durable object storage.

Upgrade migration `0025_remove_managed_email_provider.sql` moves queued legacy
messages to SMTP and marks legacy domains for Stalwart re-provisioning. On the
next verifier pass, obsolete managed-provider DNS rows are replaced by records
for the configured MTA host and IP. Historical completed provider attempts are
retained for audit accuracy.
