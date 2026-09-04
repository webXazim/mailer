# Stalwart delivery events

Mailer treats SMTP submission as queued work. It marks a recipient delivered
only after Stalwart reports `delivery.delivered`, which means the remote server
accepted that recipient. Temporary failures remain visible as `email.deferred`
events and do not become fake deliveries.

## Secrets

Generate two different secrets and store them in Mailer's production `.env`:

```env
STALWART_WEBHOOK_TOKEN=<openssl-rand-hex-32>
STALWART_WEBHOOK_SIGNING_KEY=<a-different-openssl-rand-hex-32>
```

The bearer token authenticates Stalwart to Mailer. The signing key authenticates
the exact raw request body with HMAC-SHA256. Both checks must pass. Mailer accepts
the endpoint only when both values are configured and refuses short values in
production.

## Stalwart webhook

In **Settings > Telemetry > Webhooks**, create one enabled webhook with:

- URL: `http://api:8080/internal/v1/stalwart/events`
- Events policy: `include`
- HTTP authentication: `Bearer`, using `STALWART_WEBHOOK_TOKEN`
- Signature key: `Value`, using `STALWART_WEBHOOK_SIGNING_KEY`
- Invalid certificates: disabled
- Lossy: disabled
- Timeout: `30000` ms
- Throttle: `1000` ms
- Discard after: at least `604800000` ms (seven days)

Select these events explicitly; Stalwart does not support wildcard event filters:

```text
queue.authenticated-message-queued
queue.message-queued
queue.rescheduled
queue.rate-limit-exceeded
queue.concurrency-limit-exceeded
queue.back-pressure
queue.blob-not-found
queue.quota-exceeded
delivery.delivered
delivery.failed
delivery.rcpt-to-failed
delivery.rcpt-to-rejected
delivery.message-rejected
delivery.mail-from-rejected
delivery.null-mx
delivery.connect-error
delivery.greeting-failed
delivery.start-tls-error
delivery.concurrency-limit-exceeded
delivery.rate-limit-exceeded
delivery.dsn-perm-fail
delivery.double-bounce
incoming-report.abuse-report
incoming-report.fraud-report
```

The URL works over the private `crescentsphere-mail-transport` Docker network.
Do not expose `/internal/v1/stalwart/events` through the public reverse proxy.

## Correlation and replay behavior

For SMTP delivery, Mailer writes a message ID containing both its email UUID and
provider-attempt UUID. A Stalwart event is accepted only when both identifiers
match an SMTP attempt in PostgreSQL. Events for unrelated mail on the same server
are ignored.

Stalwart batches may be duplicated, retried, or arrive out of order. Mailer uses
the Stalwart event ID plus recipient as an idempotency key. Complaint state wins
over bounce state, and a prior permanent outcome cannot be changed to delivered
by a late event. A multi-recipient email becomes delivered only after every
recipient is delivered; it stays in progress while any recipient is pending.

If processing any recognized event fails, Mailer returns an error so Stalwart
retries the batch. Events already committed from an earlier batch attempt are
safe to receive again.

## Live gate

Before routing production traffic to SMTP:

1. Send one message to each operator-controlled Gmail, Microsoft, and independent
   mailbox.
2. Confirm the API initially reports submission without claiming delivery.
3. Confirm a `delivery.delivered` event changes each recipient and then the whole
   email to delivered.
4. Send to a known nonexistent recipient and confirm a permanent bounce plus a
   suppression entry.
5. Temporarily stop the Mailer API, send a test message, restart the API, and
   confirm Stalwart retries the retained batch.
6. Replay the same signed body and confirm no duplicate delivery event or usage
   increment appears.

Keep SES SQS ingestion running for SES-routed attempts during migration. The two
adapters use separate authentication and correlation paths.
