# Current production readiness

Rechecked 2026-09-04 after the operational-safety and staged-delivery upgrades.
`READINESS_AUDIT.md` remains the historical pre-fix review.

The previously confirmed code gaps are closed and covered by a clean-database
production smoke test:

- `/operationalz` detects a stale worker heartbeat, old queued mail, stalled customer
  webhooks, and retained-content cleanup backlog.
- The worker rechecks the sender domain while creating the provider-attempt boundary.
  Disabling a domain blocks until that decision completes and stops later attempts.
- Local, exhausted, maintenance, and stale-worker failures atomically create one
  authoritative rejection event and one webhook outbox item.
- Object-store requests and response-body reads have configurable whole-operation
  deadlines.
- Retained-content cleanup drains bounded batches of up to 5,000 objects per hour,
  backs off failed objects for one hour, and exposes excessive backlog in health.

Provider routing is guarded by a global SMTP pause, a daily admission cap,
per-workspace cohorts, and pre-attempt-only SES rollback. `OPERATIONS.md` documents
monitoring, rollout, incident response, and recovery.

## Live acceptance gates still outstanding

These require the production VPS, DNS, real inboxes, and production credentials;
they cannot be established by repository tests:

- Start Stalwart with its persistent volumes, verify its public TLS certificate,
  SMTP banner, forward/reverse DNS match, outbound IPv4 path, DKIM signing, SPF,
  DMARC, and return-path alignment.
- Send controlled messages to Gmail, Outlook, and another provider. Confirm inbox
  placement, a real bounce, complaint feedback where available, suppression, signed
  webhook delivery, and retry after a temporary receiver failure.
- Upload, send, and read a real attachment through the selected S3-compatible object
  store. Confirm checksum validation, application cleanup, and a bucket lifecycle
  rule as a second safety layer.
- Exercise worker termination, PostgreSQL and NATS interruptions, Stalwart outage,
  provider timeout, disk alert, certificate-expiry alert, and ambiguous provider
  acceptance without producing a duplicate message.
- Install the health and encrypted-backup timers, connect the alert destination,
  rehearse restore of PostgreSQL plus Stalwart and object-storage data, and record
  recovery time and data-loss windows.
- Establish warm-up limits and review delivery reputation after each volume increase.
  Independent SMTP removes a runtime vendor dependency but does not remove receiver
  reputation, abuse, and blocklist dependencies.

## Product and policy work before broad promotion

MFA, team administration, billing, templates, custom headers, tags, legal policy
pages, account deletion/export, ownership-dispute handling, and automated anomaly
response remain outside the current mail-delivery core. Until automated anomaly
response is implemented, operators must review delivery reports and revoke keys,
disable domains, and pause SMTP using the documented incident procedure.

Current automated evidence is: 30 Rust tests, strict workspace Clippy, environment
and reverse-proxy regression, and a clean PostgreSQL/NATS/API production smoke test
covering migrations, routing controls, caps, worker-staleness detection, atomic local
failures, signed Stalwart events, multi-recipient convergence, replay safety, and
private-route isolation. This supports a controlled production pilot; it is not a
claim of live deliverability certification.
