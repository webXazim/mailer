# Production operations

This runbook covers the Mailer application and the independent Stalwart transport.
Use it after both Compose projects have been configured according to
`STALWART_DEPLOYMENT.md` and `DELIVERY_ROUTING.md`.

## Routine monitoring

Install and enable the health timer from `backend/deploy/systemd`. It runs every
minute and alerts when the public API is unavailable, the worker heartbeat is
stale, queued mail or customer webhooks stop progressing, retained-content cleanup
falls behind, local disk usage reaches `DISK_ALERT_PERCENT` (85 by default), or the
SMTP TLS certificate is invalid or expires within `TLS_ALERT_SECONDS` (seven days
by default).

The API endpoints have distinct meanings:

- `/healthz` proves that the API process is running.
- `/readyz` proves that PostgreSQL and NATS are reachable.
- `/operationalz` proves recent worker progress and checks delivery, webhook, and
  content-cleanup backlogs. A 503 is actionable even when `/readyz` is green.

Run `sh manage delivery-report 7` during daily review. Investigate a sudden rise in
rejects, bounces, complaints, retries, or one sender domain dominating traffic.
Review `sh manage delivery-routing-status` before and after every routing change.

## Safe transport rollout and rollback

SMTP starts paused with a daily admission cap of 100. Route one controlled
workspace to SMTP, send to operator-owned inboxes at several providers, and confirm
delivery events and a signed customer webhook. Then use `sh manage smtp-resume`.
Increase the cap in measured steps with `sh manage smtp-cap NUMBER`.

`sh manage smtp-pause` stops new SMTP provider attempts. With SES rollback enabled,
messages that have no provider attempt may move to SES. A provider attempt that has
already begun is never sent through another provider automatically because doing so
could duplicate mail. Disable a sender domain to stop its queued messages at the
final authorization boundary; an already running provider request cannot be recalled.

## Incident response

For suspected abuse or a compromised API key:

1. Revoke the key in the console and pause SMTP with `sh manage smtp-pause`.
2. Identify the workspace, sender domain, recipients, provider attempts, and event
   times using `sh manage delivery-report 1` and the API-key audit trail.
3. Disable affected domains and suppress known bad or complaining recipients.
4. Preserve database, application, Stalwart, and reverse-proxy logs before cleanup.
5. Rotate API, webhook, Stalwart admin, database, and NATS credentials if exposure
   is possible. Restart only the services that consume a rotated secret.
6. Resume a single controlled workspace under the daily cap. Confirm delivery and
   complaint handling before widening traffic.

For a worker outage, keep SMTP paused, restore worker heartbeats, and inspect stale
provider attempts. Treat an attempt left in `processing` as ambiguous and review it
manually; do not replay it automatically.

## Recovery

The backup timer creates an encrypted PostgreSQL dump every six hours and copies it
to the configured immutable remote. Run `sh manage restore-rehearsal` regularly and
record the result. PostgreSQL contains the authoritative application state; also
back up the Stalwart data/config volumes and the object-store bucket independently.

After a restore, keep sending paused. Verify database migrations, NATS access,
object retrieval and checksums, Stalwart configuration, DNS, TLS, and webhook
secrets. Run the public health check and controlled inbox delivery before resuming.
