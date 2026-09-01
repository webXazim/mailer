# Developer mailer readiness audit

> Historical pre-fix audit. Implementation work following this review is tracked in [RELEASE_NOTES.md](RELEASE_NOTES.md); the verdict below describes the earlier code, not the current private release.

Reviewed 2026-08-31 against the current working tree, including the VPS deployment changes.

**Verdict: not ready for production or customer onboarding.** The project has a substantial backend foundation and a deployable Docker stack, but the console is a prototype and several essential delivery/security paths are incomplete or incorrect. Private development testing is reasonable; exposing the current console as a finished email platform is not.

This review did not change application behavior or deploy anything to the VPS. It does not certify the entire codebase as secure. Real AWS/R2 credentials, DNS, deliverability, provider quotas, and production load were not tested.

## Confirmed launch blockers

### 1. P1 â€” Real SES feedback cannot reliably reach suppression and webhook processing

[Worker event models](E:/Framework/mailer/backend/services/worker/src/events.rs:66) expect `bounce_type`, `bounced_recipients`, and `complained_recipients`, without the necessary Serde field renaming. AWS sends camelCase fields. Reject and rendering-failure models also require fields absent from AWS payloads; the rendering event name does not match. Open/click normalization uses the original send timestamp and drops interaction details.

An isolated Rust harness compiled the existing models and normalization function unchanged: delivery passed; bounce, complaint, reject, rendering failure, and open timestamp failed. The bounce/complaint failures prevent automatic suppression, risking continued sending to bad recipients. Failed events remain for SQS redrive. Fixtures were checked against [AWS event examples](https://docs.aws.amazon.com/ses/latest/dg/event-publishing-retrieving-sns-examples.html).

**Required:** correct the provider models and normalization; retain event-specific information; add actual provider-payload tests through ingestion, suppression, and webhook creation.

### 2. P1 â€” Test and production requests share an idempotency namespace

[Email submission](E:/Framework/mailer/backend/services/api/src/emails.rs:272) looks up idempotency by workspace and key only. The [idempotency table](E:/Framework/mailer/backend/migrations/0002_sending.sql:81) has the same uniqueness rule. The request hash does not include the resolved key environment when the request omits `environment`.

**Reproduced:** submit a request with a test key, then submit identical JSON and the same idempotency key with a production key. The second call returns HTTP 200 and the test email's response. Zero production emails are queued. The reverse order also shares the namespace by construction.

**Required:** scope the database key, advisory lock, lookup, and canonical request hash by the resolved environment; migrate the existing records safely and test both directions.

### 3. P1 â€” Webhook private-network protection can be bypassed with IPv6 literals

[URL validation](E:/Framework/mailer/backend/services/api/src/webhooks.rs:463) parses `Url::host_str()` directly as an IP. Bracketed IPv6 hosts evade that check. The worker's [public DNS resolver](E:/Framework/mailer/backend/services/worker/src/webhook.rs:115) cannot compensate: the installed Hyper connector bypasses DNS resolution for literal IP addresses. IPv4-mapped IPv6 addresses are also not normalized by either IP filter.

**Reproduced:** `https://127.0.0.1/events` is rejected with 400; `https://[::1]/events` and `https://[::ffff:127.0.0.1]/events` are accepted with 200. The connector's literal-address bypass was verified in cached dependency source. No private service was contacted; HTTPS certificate validation remains a separate restriction on completing an HTTP request.

**Required:** validate typed hosts, normalize mapped addresses, and enforce the allowed destination addresses at connection time for both literal and DNS hosts. Test loopback, private/link-local ranges, and DNS rebinding.

### 4. P1 â€” API-key rotation can succeed without revoking the previous key

[Rotation](E:/Framework/mailer/backend/services/api/src/api_keys.rs:174) inserts the replacement and revokes the old key in separate database operations. The revocation error is discarded. Concurrent rotations are also not serialized.

**Reproduced:** a disposable database trigger rejected revocation. The endpoint still returned HTTP 200 and a replacement key, while the old key remained active.

**Required:** perform the lookup/lock, replacement, and revocation in one transaction; return success only after commit. Test failure and concurrent rotation.

### 5. P1 â€” The dashboard and authentication screens are simulations

[AuthPage](E:/Framework/mailer/frontend/src/features/auth/AuthPage.tsx:9) navigates on submit without authentication requests. The MFA screen accepts any syntactically valid six-digit code. [Key creation](E:/Framework/mailer/frontend/src/features/api-keys/ApiKeysPage.tsx:22) generates a local `Math.random()` string instead of a backend key. [Send test](E:/Framework/mailer/frontend/src/App.tsx:82) only changes local UI state. Domains, email activity, webhooks, usage, and other screens use fixtures/local state.

The API client exists, but the feature screens do not use it. This is a product integration gap, not evidence that the backend's session/API-key checks can be bypassed. The existing API still rejects unauthorized requests.

**Required:** connect signup/login/session/logout, domain setup, real API keys, sending, activity, and webhooks to the backend. Add route/session handling and honest loading/error states. Implement MFA or remove the claim and simulated flow.

### 6. P1 â€” Password recovery is not delivered; account verification is absent

[Password reset](E:/Framework/mailer/backend/services/api/src/auth.rs:396) generates a token, stores its hash, logs that delivery will be added later, and discards the raw token. It never queues reset mail. The frontend nevertheless says instructions were sent. Email verification and MFA are not implemented as usable backend flows.

**Reproduced:** reset returns 200 without adding a mail job. Code inspection confirms there is no alternate delivery path.

The [reset-completion handler](E:/Framework/mailer/backend/services/api/src/auth.rs:405) also performs synchronous password hashing before checking whether the token exists, without the rate limiting used by other auth routes. This exposes expensive work to unauthenticated requests.

**Required:** deliver expiring reset links through an account-mail path; invalidate outstanding reset tokens appropriately; add verified-account onboarding before public signup, abuse limits, and bounded password-hashing work.

### 7. P1 â€” SES event publishing is not connected to sends by the application

Neither the [simple send](E:/Framework/mailer/backend/services/worker/src/delivery.rs:310), [raw send](E:/Framework/mailer/backend/services/worker/src/delivery.rs:362), nor [identity creation](E:/Framework/mailer/backend/services/api/src/domains.rs:103) selects a configuration set. No configuration-set setting is present. Creating the SNS/SQS destination alone does not attach it to messages.

This is a code/setup gap, not a claim about the unseen AWS account: a manually assigned default configuration set on each identity can compensate. The application does not establish or validate that prerequisite. See [AWS configuration-set selection](https://docs.aws.amazon.com/ses/latest/dg/using-configuration-sets-in-email.html).

**Required:** select a configured set on every send, or configure and verify an identity default during onboarding. Prove delivery, bounce, and complaint events arrive from actual SES simulator sends before launch.

### 8. P1 â€” Provider retry and failure recovery can lose reliable state

[Provider error classification](E:/Framework/mailer/backend/services/worker/src/delivery.rs:463) searches `SdkError::to_string()` for throttling details. The installed AWS SDK formats modeled service errors as the generic string `service error`; timeout errors are formatted `request has timed out`, which also misses the current `timeout` check. Consequently, exhausted SDK throttling retries become permanent failures instead of entering the queue's intended retry path. This was verified against the actual cached SDK source.

The [terminal failure branch](E:/Framework/mailer/backend/services/worker/src/delivery.rs:149) acknowledges the job after helpers that discard database write errors. If persistence fails, the queue job can be acknowledged without a durable failed/dead-letter record. Stale processing reconciliation runs only at [worker startup](E:/Framework/mailer/backend/services/worker/src/main.rs:44). Shutdown aborts tasks instead of draining in-flight sends.

**Required:** classify typed SDK errors and explicitly control retry behavior for ambiguous sends; persist terminal state before acknowledging; run periodic reconciliation; provide an operator recovery path and test crash/DB-failure/restart scenarios. Do not blindly resend messages whose provider acceptance is uncertain.

### 9. P1 â€” Developers cannot reliably retrieve or correlate email outcomes

[Email routes](E:/Framework/mailer/backend/services/api/src/emails.rs:51) expose only POST. There is no list or retrieve endpoint. The [webhook body](E:/Framework/mailer/backend/services/worker/src/webhook.rs:222) contains a delivery-event ID and normalized provider payload, but not the application email ID returned when submitting mail. The normalized payload's `messageId` is the SES ID; there is no public lookup mapping it back to the submitted email. Its top-level event type is `delivery`, while the subscription name is `email.delivery`.

**Reproduced:** GET `/v1/emails` returns 405 and GET `/v1/emails/{id}` returns 404. Payload/correlation defects were established by tracing event storage and webhook serialization.

**Required:** implement tenant- and environment-scoped email retrieval/activity with pagination and events. Include the public email ID, environment, stable event names, and useful metadata in a documented webhook contract.

## Other correctness and completeness gaps

| Priority | Finding | Evidence and needed change |
| --- | --- | --- |
| P2 | Advertised scopes are unusable outside sending | A key with `domains:read`, `webhooks:manage`, and `workspace:read` receives 401 on those endpoints, which require session cookies. `emails:read` has no route. [Allowed scopes](E:/Framework/mailer/backend/services/api/src/api_keys.rs:16) need matching authorization/routes or removal until supported. |
| P2 | API accepts more recipients than SES can send | [MAX_RECIPIENTS](E:/Framework/mailer/backend/services/api/src/emails.rs:18) is 100; the worker sends one SES message. A 51-recipient submission returned 202. Limit to 50 or implement well-defined splitting with individual IDs/status. [AWS SES recipient limit](https://aws.amazon.com/blogs/messaging-and-targeting/how-to-send-messages-to-multiple-recipients-with-amazon-simple-email-service-ses/). |
| P2 | Accepted custom headers and tags are silently ignored | They are persisted by submission, but the [worker claim/query](E:/Framework/mailer/backend/services/worker/src/delivery.rs:175) never loads them and neither send path applies them. Implement validated forwarding or reject unsupported fields. |
| P2 | Test mode stops short of a useful delivery simulator | [Test sends](E:/Framework/mailer/backend/services/worker/src/delivery.rs:276) receive a fake provider ID and `sent` status, but no synthetic feedback/webhooks or terminal timestamp. [Content retention](E:/Framework/mailer/backend/services/worker/src/lifecycle.rs:20) only covers terminal statuses, so these blobs do not expire through this job. Add deterministic test outcomes and a retention policy for nonterminal/orphaned content. |
| P2 | Usage and plan reporting are placeholders | [Workspace response](E:/Framework/mailer/backend/services/api/src/auth.rs:499) always reports free/0 sent/1000 limit. Actual defaults permit 100000 accepted messages; counters count submissions rather than recipients and are shared across test/production. Define the accounting model and return actual usage/limits before advertising quotas or billing. |
| P2 | Domain lifecycle lacks reconciliation | SES identity creation precedes the DB transaction. Several subsequent errors can leave an external identity without a local domain; retries create again instead of adopting/reconciling it. Globally reserved pending domains have no expiry/ownership-dispute flow. Add idempotent provisioning and recovery before open registration. |
| P2 | Templates, suppression management, billing, and team workflows are incomplete | GET `/v1/templates`, `/v1/suppressions`, and `/v1/billing` returned 404. Schema/UI presence is not a working feature. Implement the features chosen for release; hide/defer the others. Billing, advanced templates, and team management need not block a private send-only MVP. |

## What is already present

- Rust API and worker, PostgreSQL migrations, authenticated NATS, transactional email acceptance/outbox, hashed API-key storage, session cookies/password hashing, and workspace checks.
- SES domain/DKIM/custom MAIL FROM setup and DNS verification code; text/HTML, recipients, reply-to, and attachment delivery paths.
- Object-storage/checksum support, feedback ingestion, suppression tables, webhook signing/retries/attempt history, quotas, and maintenance foundations. Their presence does not negate the defects above.
- Docker builds and local production-mode readiness/proxy/migration smoke tests passed during deployment preparation. The current audit additionally verified real signup, key issuance, and same-environment idempotent replay with a disposable database.

## Verification and limits

The API probes used locally built API/frontend images with a new isolated Postgres/NATS stack, fake credentials, development mode, and provider integrations disabled. A verified domain was seeded directly solely to exercise submission. No worker, Cloudflare connector, real email, or outgoing webhook ran. The fault-injection trigger and all test containers/volumes were removed.

The isolated SES parser harness copied the existing models and normalization code verbatim and ran six focused compatibility tests: **1 passed, 5 failed**. These are defect reproductions, not a passing regression suite. The full Rust test suite, end-to-end AWS flow, production R2 behavior, browser integration, tenant-isolation penetration testing, and load/recovery testing were not run in this audit.

## Release sequence

1. Fix event parsing/publishing, environment isolation, webhook destination safety, key rotation, and worker failure persistence/retries.
2. Finish the real console/account flow and the minimum developer API: send, retrieve/status, domains, keys, suppressions, and correlated signed webhooks. Remove unsupported feature promises.
3. Run automated tenant/role/environment tests and provider-shaped event tests; verify actual SES simulator delivery/bounce/complaint flows and R2 attachment/retention behavior in staging.
4. Exercise worker restart, provider throttling, database/queue interruption, webhook retries, and backup restore on the VPS before enabling customer traffic.

For your own applications, a narrower private MVP is achievable without billing or advanced team/template features. The safety and delivery defects still need fixing first.
