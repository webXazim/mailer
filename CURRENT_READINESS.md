# Remaining readiness work

Rechecked 2026-08-31 after the private-release implementation. This is the current
assessment; `READINESS_AUDIT.md` is the historical pre-fix review.

The core private developer workflow is implemented and locally tested. Production
readiness is not yet established. Passing unit/integration tests does not establish
provider delivery, operational recovery, or coverage of every failure path.

## Confirmed code and operations gaps

| Priority | Gap | Evidence and consequence | Required completion |
| --- | --- | --- | --- |
| P1 | Sending can stop while public health stays green | `backend/services/api/src/main.rs:176` checks PostgreSQL and NATS, not worker progress. The production worker has no health check. `backend/deploy/healthcheck.sh` checks HTTP endpoints only. | Worker/task progress reporting and stale queue/event-ingestion alerts; prove detection by stopping or stalling the worker. |
| P1 | Domain disable is not a queue stop | `backend/services/api/src/domains.rs:301` disables the domain locally. `backend/services/worker/src/delivery.rs:291` checks suppressions before sending but never rechecks the domain. Previously accepted messages may still send. | Recheck sender-domain authorization before provider submission; test disable after acceptance. Already in-flight provider requests cannot be recalled. |
| P2 | Local send failures have no webhook event | `backend/services/worker/src/delivery.rs:470` persists failure without a delivery event/outbox entry. Maintenance failures have the same limitation. Provider feedback events are supported, but applications relying solely on webhooks can miss local failures. | Persist a documented failure event and outbox entry atomically, including exhausted/stale jobs. Until then poll email status. |
| P2 | Content cleanup has a low fixed ceiling | `backend/services/worker/src/lifecycle.rs:7` runs hourly and selects at most 100 objects. At most 2,400 objects/day are removed in steady state, below the average daily volume allowed by the default 100,000 monthly submissions. Failed deletes reduce this further. | Drain bounded batches with fair progress and backlog monitoring; verify R2 lifecycle rules as a safety net. |
| P2 | Object-storage calls lack an explicit application deadline | `backend/crates/storage/src/lib.rs` sets no total operation timeout; body collection is also unbounded by an application timeout. The sole delivery loop awaits content retrieval before sending. SDK defaults do not establish the desired whole-operation bound. | Bound storage request/body duration and exercise slow/unavailable storage, including shutdown and recovery. |

These are code-inspection findings. This follow-up did not reproduce failure cases,
change application behavior, or rerun the earlier test suites.

## Provider and VPS acceptance checks still outstanding

- Configure actual SES/SNS/SQS and R2 credentials, IAM, configuration set, account
  sender, sender DNS, and SES account access/quotas. Those values are not verified
  by a passing configuration preflight.
- Send to an inbox under operator control; confirm actual delivery, bounce and
  complaint feedback, suppression, and a signed HTTPS webhook received and verified
  by the consuming application. Verify retry after a transient receiver failure.
- Upload/send/read an attachment through the real object store and confirm retention.
- Deliver a password-reset email through SES and complete the browser reset flow.
- Deploy behind the real tunnel; check HTTPS cookies, forwarding, private-route
  blocking, resource headroom beside Docgen/Messenger, and a normal redeploy.
- Exercise database/NATS interruptions, provider throttling, worker termination
  during sending, and ambiguous provider acceptance. The current tests do not
  cover these end to end.
- Configure and rehearse encrypted offsite recovery, including PostgreSQL, NATS,
  and R2. Verify alerts and SQS dead-letter handling. No VPS load or restore
  rehearsal has been performed in this session.

## Deliberately absent features

Public email-verified signup, MFA, team administration, billing, templates, custom
headers, and tags remain absent. They need not block trusted private use, but the
service should not be opened as a public multi-customer platform on that basis.
Unverified domain reservations also lack expiry/ownership-dispute administration.

The previous passing checks remain useful evidence: 18 Rust tests, frontend
production build, isolated API/NATS/PostgreSQL integration, strict production API
startup with dummy settings, and browser flows. They support controlled testing;
they are not a production certification.
