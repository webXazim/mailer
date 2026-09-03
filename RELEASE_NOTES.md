# Production self-service update

See [the current readiness assessment](CURRENT_READINESS.md) for remaining gaps
identified in the follow-up review before relying on production delivery.

This release turns the prototype console into a self-service developer mailer.
Real delivery is available after configuring providers and granting the SES account
production access in the configured region.

## Available workflows

- Public signup with Turnstile, email verification, automatic workspaces, login,
  logout, and queued email password recovery.
- Test and production API keys, permissions, expiry, atomic rotation, and revocation.
- Domain registration, ownership/DKIM/MAIL FROM DNS instructions, verification,
  and local disabling without deleting shared SES identities.
- Email submission, retained content, recipients, metadata, status, and event history.
  Test sends simulate delivery/bounce/complaint without contacting recipients.
- Text/HTML, reply-to, attachments, and at most 50 combined To/CC/BCC recipients.
- Environment-specific signed webhooks, attempt history, secret rotation, and retry.
- Workspace suppression management and checks at admission and before delivery.

The console uses persisted API data. Prototype billing, MFA, team administration,
and template screens are no longer mounted. Those features,
custom headers, and tags are not part of this release.

## Reliability and security changes

Idempotency now separates test and production. API keys cannot cross email
environments or fall back to a browser session when authentication fails.
Webhook requests reject unsafe destinations, including DNS answers on private
networks. Key rotation rolls back entirely if revocation fails.

Every real developer send selects `SES_CONFIGURATION_SET`. SES event parsing
handles provider event shapes, correlates application IDs/metadata, and routes
webhooks to the matching environment. Worker shutdown drains active sends;
stale or exhausted jobs become visible failures. Ambiguous SES acceptance is
held for operator reconciliation instead of automatically risking a duplicate.
Content retention also applies when final delivery feedback never arrives.

Migration `0015_public_signup.sql` adds verification tokens and per-workspace production
access. Migration `0017_verified_domain_production_access.sql` unlocks production for
workspaces with a verified sending domain. Back up before deployment.
Keep existing DB/NATS/webhook master secrets stable across redeployments.

## Verification

The release checks passed: 18 Rust unit tests, the TypeScript/Vite production build,
and an isolated Docker integration suite with real PostgreSQL, NATS, API, and worker.
The suite covers authentication, tenant/scope/environment boundaries, idempotency,
rotation rollback under a database fault, suppression, simulated delivery/bounce,
event deduplication, webhook routing, and one-time password resets. The final phase
also passed strict production API startup, migrations, authenticated NATS, and
private-route blocking with dummy provider settings and the worker stopped.

Browser checks cover login, test submission through delivered status, content/event
inspection, key creation, and management screens. No real provider credentials are
used by the integration suite. It does not prove external SES, R2, SQS, DNS, HTTPS
webhook delivery, or account-email delivery.

Reproduce from the repository root:

```bash
docker build -f backend/deploy/tests/rust.Dockerfile backend
docker build --target api -t mailer-usable-api:local backend
docker build --target worker -t mailer-usable-worker:local backend
docker build --build-arg NGINX_CONFIG=nginx.production.conf -t mailer-usable-frontend:local frontend
python backend/deploy/tests/integration.py
```

## Before using production keys

1. Fill provider credentials at the top of `.env`, including the SES configuration
   set, `ACCOUNT_EMAIL_API_KEY`, a verified `ACCOUNT_EMAIL_FROM`, and Turnstile keys.
2. Configure SES/SNS/SQS event delivery, IAM, R2 retention, and sender DNS. Review
   SES sandbox/production access and account sending quotas.
3. Run `sh manage deploy` and configure the Cloudflare route to `http://api:8081`.
4. Perform a real send to an inbox you control, check delivery/bounce/complaint
   feedback, receive and verify an HTTPS webhook, retrieve an attachment from R2,
   and complete a password reset delivered by SES.
5. Rehearse backups/recovery and monitor queue failures, resources, SES reputation,
   and account limits. Rotate the tunnel token shared in chat before customer use.

The monthly limit is an accepted-message counter, including tests, rather than
a billing or recipient meter. Suppressions are shared across workspace environments.
Management permissions apply to shared workspace resources. This release does not
claim zero downtime, exactly-once provider delivery, or automatic rollback.

See [deployment instructions](README.md) and [API/recovery details](backend/README.md).
