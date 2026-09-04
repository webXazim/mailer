# Self-hosted mail transport roadmap

## Goal

Make CrescentSphere Mailer capable of delivering transactional email through a
self-hosted Stalwart MTA on the Netcup server, without requiring Amazon SES for
delivery, domain provisioning, or delivery events. Keep SES as an optional,
explicitly selected provider during migration and for emergency rollback.

This removes the managed email-delivery dependency. It does not remove the
unavoidable dependencies on the IP/network operator, public DNS, domain registrar,
certificate authority, and recipient mail systems. R2 replacement is a separate
infrastructure-independence milestone.

## Target architecture

```text
Developer API
    -> PostgreSQL outbox -> NATS -> Mailer worker
                                      |
                                      +-> SES adapter (optional)
                                      |
                                      +-> SMTP adapter -> Stalwart queue
                                                            |
                                                            +-> recipient MX servers

Stalwart signed webhook -> Mailer event-ingestion endpoint
                              -> email state / suppressions / developer webhooks
```

PostgreSQL remains the product source of truth. Stalwart owns SMTP submission,
DKIM signing, remote delivery attempts and its own delivery queue. An SMTP `250`
response from Stalwart means queued, not delivered.

## Upgrade 0: network identity and host baseline

**Outcome:** the server is safe to identify itself as `smtp.crescentsphere.com`.

- Confirm forward DNS: `smtp.crescentsphere.com -> 152.53.178.165`.
- Confirm PTR: `152.53.178.165 -> smtp.crescentsphere.com`.
- Confirm outbound IPv4 TCP 25.
- Use IPv4 only at first. Do not send over IPv6 until a selected IPv6 address has
  matching AAAA, PTR and SPF authorization.
- Confirm time synchronization, hostname, disk capacity and backup destination.
- Inventory ports already used by Docgen, Messenger, Mailer and Docker.
- Preserve SSH access before applying an allow-list firewall policy.

**Gate:** forward and reverse DNS match from two public resolvers, and an outbound
IPv4 SMTP connection succeeds.

## Upgrade 1: isolated Stalwart deployment

**Outcome:** a pinned Stalwart instance accepts authenticated submission but is not
yet used by customers.

- Run Stalwart as a separate Compose project with its own configuration and data
  volumes. Do not merge its lifecycle with the Mailer application stack.
- Pin a reviewed Stalwart minor/image digest; do not deploy `latest`.
- Set the server hostname to `smtp.crescentsphere.com`.
- Expose SMTP 25 directly. Expose authenticated submission on 465 and optionally
  587. Do not route SMTP through Cloudflare Tunnel.
- Keep the admin interface private through localhost, VPN, or an authenticated
  tunnel. Do not expose port 8080 to the public internet.
- Use ACME DNS-01 through a narrowly scoped Cloudflare token if host port 443 is
  already occupied.
- Persist and back up Stalwart configuration, queue/state, keys and credentials.
- Create a dedicated submission principal for the Mailer worker. It must not have
  administrative permissions or act as an open relay.

**Gate:** authenticated TLS submission works locally; unauthenticated Internet
relay attempts fail; restart and restore tests retain configuration and queued mail.

## Upgrade 2: sending-domain authentication

**Outcome:** Stalwart mail passes SPF, DKIM and DMARC without disturbing SES.

- Keep `smtp.crescentsphere.com` DNS-only in Cloudflare.
- Generate a Stalwart-specific DKIM selector. Do not replace SES selectors while
  SES remains available.
- Use a distinct direct-delivery envelope/return-path subdomain so SES and
  Stalwart MX/SPF records do not conflict.
- Publish exactly one SPF TXT record per hostname. Authorize only the relevant
  IP/provider in each record.
- Keep DMARC at `p=none` while collecting reports, then move deliberately to
  `quarantine` and finally `reject` after alignment is proven.
- Add TLS-RPT and MTA-STS only after HTTPS policy hosting and renewal are tested.
- Prevent customer DKIM CNAMEs from being Cloudflare-proxied.

**Gate:** test messages show SPF=pass, DKIM=pass, DMARC=pass and TLS from Gmail,
Outlook and another independent receiver.

## Upgrade 3: provider-neutral delivery code

**Outcome:** the worker can select SES or Stalwart without changing the public API.

**Local implementation status:** provider selection, shared MIME generation,
authenticated TLS SMTP submission, per-message routing, provider-attempt storage,
configuration validation, and account-email routing are implemented. The live
Stalwart acceptance/rejection/timeout suite is still required before this gate is
complete or production traffic moves from SES.

- Introduce a `DeliveryProvider` boundary with SES and SMTP implementations.
- Replace production's hard requirement for `DOMAIN_PROVIDER=ses` with explicit
  delivery and domain-management settings.
- Reuse one MIME builder for HTML, text, reply-to and attachments across providers.
- Add provider type, submission/queue identifier and attempt identifier to stored
  delivery state. Do not assume every provider ID is globally unique.
- Classify SMTP failures into transient, permanent and ambiguous outcomes.
- Treat a lost connection after message transmission as ambiguous; never blindly
  retry and risk a duplicate.
- Support provider selection per environment/workspace through operator policy,
  not an untrusted request field.
- Add configuration similar to:

```env
DELIVERY_PROVIDER=smtp
SMTP_HOST=smtp.crescentsphere.com
SMTP_PORT=465
SMTP_SECURITY=implicit_tls
SMTP_USERNAME=mailer-worker
SMTP_PASSWORD=replace-with-secret
SMTP_HELO_NAME=smtp.crescentsphere.com
SMTP_TIMEOUT_SECONDS=30
```

**Gate:** the same integration suite passes with SES and SMTP adapters, including
attachments, multi-recipient mail, timeouts, rejection and ambiguous acceptance.

## Upgrade 4: domain provisioning independent of SES

**Outcome:** new users can verify and configure domains without AWS APIs.

**Local implementation status:** the provider-neutral schema, Stalwart JMAP
domain/key provisioning, manual and Cloudflare DNS publication, public-DNS
verification, aligned return path, DKIM rotation, and MTA disable flow are
implemented. The live clean-workspace gate on the VPS is still required.

- Replace SES-specific domain fields and DNS generation with a provider-neutral
  domain model.
- Retain Mailer's random ownership TXT record.
- Generate/publish Stalwart DKIM, return-path SPF/MX and DMARC instructions.
- Integrate Stalwart's management API for domain/key lifecycle, or use an internal
  reconciler with least-privilege credentials.
- Keep Cloudflare OAuth publishing optional. Manual DNS instructions must continue
  to work with every DNS provider.
- Reconcile partial failures so a domain created in Stalwart but not committed in
  PostgreSQL is adopted or cleaned up safely.

**Gate:** a clean workspace can add a domain, publish records manually or through
Cloudflare, verify it, rotate DKIM and disable sending without AWS credentials.

## Upgrade 5: authoritative delivery-event pipeline

**Outcome:** the UI and customer webhooks report actual outcomes rather than SMTP
submission success.

- Configure a Stalwart webhook with a dedicated HMAC key and bearer/basic
  authentication to a private Mailer ingestion endpoint.
- Ingest queued, delivered, deferred, permanently failed and DSN-related events.
- Correlate events using a Mailer-controlled message/attempt identifier embedded in
  the submitted message and retained in provider metadata.
- Make event ingestion idempotent and safe for batches, reordering and duplicates.
- Only mark `delivered` from an authoritative Stalwart delivery event.
- Feed permanent bounces and validated complaints into workspace suppressions.
- Extend webhook retention beyond the short default so a Mailer outage does not
  silently lose delivery state, or add a reconciliation poll against Stalwart's
  queue/history API.
- Keep SES SQS ingestion as a separate adapter while SES is enabled.

**Gate:** queued, delivered, deferred and failed messages converge correctly after
worker/API restarts and after webhook loss/retry simulations.

## Upgrade 6: routing, rollback and reputation controls

**Outcome:** controlled migration without duplicate delivery or rapid IP damage.

- Start with internal authentication and notification email only.
- Add operator-controlled cohorts and daily volume caps for Stalwart.
- Warm the IPv4 gradually across Gmail, Microsoft and other mailbox providers.
- Track acceptance, deferral, bounce, complaint and spam-placement rates by domain.
- Use explicit routing: SES or Stalwart is chosen before an attempt begins.
- Fail over only before provider acceptance. Never resend a permanent rejection or
  ambiguous accepted attempt through the other provider.
- Provide a global Stalwart pause switch and an SES rollback switch.
- Throttle per recipient domain and honor remote `4xx` retry guidance.

**Gate:** a staged cohort operates within defined error/reputation thresholds for
at least two weeks, and rollback is rehearsed without duplicates.

## Upgrade 7: abuse, security and operations

**Outcome:** public developer access cannot turn the host into a spam source.

- Enforce verified domains, per-workspace quotas, recipient/concurrency limits and
  rate limits at both API and MTA layers.
- Detect compromised API keys, sudden volume changes, enumeration and repeated
  invalid recipients. Add automated pause/review controls.
- Maintain global and workspace suppressions; process abuse reports and complaints.
- Monitor Mailer worker progress, NATS backlog, Stalwart queue age/depth, disk,
  certificate expiry, webhook failures and outbound rejection rates.
- Complete the existing P1/P2 readiness gaps in `CURRENT_READINESS.md`.
- Rehearse encrypted restore of PostgreSQL, NATS, R2 and Stalwart state.
- Maintain patch cadence and a documented incident/abuse response procedure.

**Gate:** alert and restore drills pass, an intentionally compromised test key is
contained, and an independent security/relay review finds no open relay or exposed
administration path.

## Upgrade 8: optional full infrastructure independence

**Outcome:** AWS SES and Cloudflare R2 are optional rather than required at runtime.

- Remove AWS credentials, SES SDK startup requirements, SQS and SNS after the
  Stalwart migration gates pass.
- Replace R2 with a self-hosted S3-compatible store such as MinIO, or store content
  in a separately backed-up Stalwart/Mailer data tier.
- Keep an external secondary DNS provider; authoritative DNS redundancy is safer
  than hosting the only DNS service on the mail server.
- Move Stalwart to a dedicated mail host/IP before material customer volume so
  application incidents and builds cannot affect the MTA.

**Gate:** a deployment with no AWS credentials completes domain onboarding, sends,
receives delivery outcomes, processes suppression, serves retained content and
passes backup restoration.

## Recommended release sequence

| Release | Scope | Production traffic |
| --- | --- | --- |
| R1 | Upgrades 0-2 | None; operator test recipients only |
| R2 | Upgrades 3 and 5 | Internal account/auth email |
| R3 | Upgrade 4 | New domains available in controlled beta |
| R4 | Upgrade 6 | Small opt-in customer cohort |
| R5 | Upgrade 7 | Broader production after operations review |
| R6 | Upgrade 8 | Remove AWS runtime dependency |

The work should be considered six releases with eight technical upgrades. The
critical path is not installing Stalwart; it is provider-neutral domain handling,
authoritative event correlation, abuse prevention and reputation-safe rollout.

## References

- Stalwart Docker deployment: <https://stalw.art/docs/install/platform/docker/>
- Stalwart DNS records: <https://stalw.art/docs/install/dns/>
- Stalwart server security and ports: <https://stalw.art/docs/install/security/>
- Stalwart webhook telemetry: <https://stalw.art/docs/telemetry/webhooks/>
- Stalwart event catalogue: <https://stalw.art/docs/ref/events/>
- Netcup firewall behavior: <https://www.netcup.com/en/helpcenter/documentation/server/firewall>
