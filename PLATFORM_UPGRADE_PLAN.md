# Mailer platform upgrade plan

## Product contract

Applications integrate with one provider-neutral API. They never receive SES or
SMTP credentials and never select a transport in an email request. A production
submission uses a verified sender domain, a server-side API key, and a stable
idempotency key. The accepted message stores its selected provider before it is
queued, so later operator changes cannot duplicate or reroute it.

Mailer itself uses the same contract for signup verification and password reset.
Those messages are submitted with `ACCOUNT_EMAIL_API_KEY`; therefore the workspace
owning that key is the explicit system-email cohort.

## Phase 1: reliable developer path

Status: implemented locally; deploy and verify.

- One `POST /v1/emails` contract for SES and SMTP.
- Test and production keys with scoped permissions.
- Required idempotency and safe retry guidance.
- Verified-domain enforcement, suppressions, quotas, retained content, status
  polling, and signed webhooks.
- Console examples for cURL, Node.js, and Python, including transactional-email
  retry semantics and the distinction between `sent` and `delivered`.

Acceptance:

1. A sample service sends verification, password-reset, receipt, and alert mail
   using stable event-specific idempotency keys.
2. The same code succeeds while the operator changes the route from SES to SMTP
   and back.
3. Webhook consumers deduplicate events and reach terminal message state.

## Phase 2: safe runtime provider switching

Status: implemented locally; deploy and rehearse.

- `DELIVERY_PROVIDER` remains the boot-time fallback.
- `default-provider ses|smtp|environment` changes the route for newly accepted
  default traffic without an environment edit or service restart.
- Workspace routes override the runtime default for cohorts.
- SMTP pause, daily cap, and pre-attempt SES rollback remain independent controls.
- Messages with a provider attempt are never crossed over to another provider.
- The monitor checks SMTP TLS whenever SMTP is configured, including mixed mode.

Acceptance:

```sh
sudo sh manage delivery-routing-status
sudo sh manage default-provider smtp
sudo sh manage default-provider ses
sudo sh manage default-provider environment
```

For each switch, submit a new uniquely identified message and confirm its stored
`deliveryProvider`. Confirm a message accepted before the switch keeps its original
provider. Rehearse `smtp-pause` with SES rollback enabled and disabled.

## Phase 3: automatic multi-domain Stalwart identity

Status: server policy documented; live policy update required.

Stalwart must derive the DKIM signing identity from Mailer's return path instead
of using a hard-coded domain. For `bounce.<domain>`, its ordered rule strips the
`bounce.` prefix and selects the provisioned signatures for `<domain>`. A second
condition preserves ordinary local-domain signing. Mailer continues to provision
the domain, bounce alias, DKIM key, SPF/MX return path, DMARC record, and ownership
record.

Acceptance for every onboarded domain:

- Received headers show SPF, DKIM, and DMARC pass.
- DKIM `d=` equals the visible sender domain.
- Return-Path uses `bounce.<domain>` and accepts a real DSN.
- Rotation preserves the old key until the new record is publicly visible.

## Phase 4: system email rollout

Status: application path exists; production cohort verification required.

1. Keep a dedicated workspace and least-privilege production key for
   `ACCOUNT_EMAIL_API_KEY`.
2. Verify `ACCOUNT_EMAIL_FROM` under a dedicated transactional domain.
3. Route only that workspace to SMTP first.
4. Check signup verification and password reset with
   `sh manage account-email-status ADDRESS`.
5. Keep SES configured until SMTP delivery and webhook convergence are stable.

## Phase 5: reputation and operations gate

Status: ongoing production work.

- Warm the dedicated IP gradually across Gmail, Microsoft, Yahoo, and another
  independent receiver.
- Track delivery, deferral, bounce, complaint, queue age, webhook failures, and
  spam placement by provider and recipient domain.
- Complete relay, outage, duplicate-prevention, backup, and restore drills.
- Keep customer cohorts capped until authentication passes and placement is stable
  for at least two weeks.

Broad customer traffic is ready only after these live gates pass. SMTP acceptance
alone is insufficient; authoritative delivery events and inbox placement are the
release criteria.
