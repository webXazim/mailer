# Customer SMTP submission

Mailer offers authenticated SMTP as an optional ingress beside `POST /api/v1/emails`.
Both use the same API key, verified sending domains, suppression checks, quotas,
durable database/outbox, worker, Stalwart transport, delivery events and webhooks.
The SMTP `250 queued` response means Mailer committed the job, not that a recipient
server delivered it. Customer-facing SMTP is **not** Stalwart's mailbox submission.

## Hostnames and ports

`smtp.crescentsphere.com` is the existing Stalwart transport/mailbox server on
`152.53.178.165` and owns public ports 25, 465, 587 and 993. Keep it running.
The Mailer gateway uses `smtp.mailer.crescentsphere.com`. Add a DNS-only A record
for the gateway address. This submission hostname needs no MX record.

There are two supported public address plans:

1. **Standard ports:** Add a second routed public IPv4 address to the VPS (or a
   separate host), then set `MAILER_SMTP_BIND_IP` to that address and the published
   ports to 465 and 587. The Stalwart Compose file binds its existing ports to
   `STALWART_IPV4`, allowing this separate listener. Apply the Stalwart Compose
   binding change in a scheduled restart and confirm mailbox service before
   publishing the gateway.
2. **Current IP:** Point the gateway hostname to `152.53.178.165` and use 2465
   and 2587 as the published ports. Clients must explicitly configure those
   ports. This can run without another IP and without changing Stalwart's ports.

The defaults bind only `127.0.0.1:1465` and `127.0.0.1:1587` for local testing.
Cloudflare's ordinary HTTP proxy/Tunnel cannot publish standard SMTP for arbitrary
mail clients; keep the SMTP A record DNS-only. Permit only the chosen published
ports through the VPS and provider firewalls.

## Enable on the VPS

After pulling the code into `/srv/apps/mailer`, run `sh manage production-env-upgrade`.
Obtain a trusted TLS certificate for `smtp.mailer.crescentsphere.com` (DNS-01 is
appropriate if port 80 is occupied). The existing Cloudflare DNS Certbot plugin
and its private token file can issue it:

```bash
sudo certbot certonly --dns-cloudflare \
  --dns-cloudflare-credentials /etc/letsencrypt/cloudflare/dns.ini \
  -d smtp.mailer.crescentsphere.com
sudo sh smtp-gateway/install-certificate.sh
sudo install -m 0755 smtp-gateway/install-certificate.sh \
  /etc/letsencrypt/renewal-hooks/deploy/cs-mailer-smtp.sh
```

The hook copies the certificate and key into the Git-ignored `secrets/smtp`
directory with restrictive permissions and restarts the gateway after renewals.
Reinstall the hook if the repository script changes in a future release. Run
`sudo certbot renew --dry-run` to check renewal.

Set these values in the private `/srv/apps/mailer/.env`:

```dotenv
SMTP_GATEWAY_ENABLED=true
SMTP_GATEWAY_SHARED_SECRET=<different output from openssl rand -hex 32>
SMTP_GATEWAY_TLS_DIR=./secrets/smtp
MAILER_SMTP_BIND_IP=127.0.0.1
MAILER_SMTP_IMPLICIT_PORT=1465
MAILER_SMTP_STARTTLS_PORT=1587
```

Change the bind IP and ports as described above only when ready for public access.
The gateway secret authenticates the gateway to Mailer's private API. It is not
a customer password, and must differ from the Stalwart password and other keys.
Then run `sh manage preflight`, `sh manage deploy`, and `sh manage production-status`.
Deployment builds the gateway before replacing running services. Check
`sh manage production-logs smtp_gateway` if it does not become healthy.

## Customer settings

| Setting | Value |
| --- | --- |
| Server | `smtp.mailer.crescentsphere.com` |
| Port/security | `465` implicit TLS or `587` STARTTLS on a second IP; otherwise the configured alternate ports |
| Username | `mailer` |
| Password | A production Mailer API key with `emails:send` scope |
| Sender | An address in that key's workspace's verified domain |

Create a separate scoped key for each application. A test key simulates sending;
it does not contact Stalwart. Revoking/rotating a key also blocks new SMTP AUTH
and submissions. Never put a key in frontend code or Git.

The gateway supports SMTP AUTH PLAIN and LOGIN **only over TLS**, text and HTML
MIME, and up to 10 attachments within the existing API limits. The envelope
sender must match the MIME `From` address. SMTP envelope recipients are used for
delivery, including Bcc. Unsupported MIME structures are rejected; arbitrary
custom message headers are not preserved by the current API. Message size is
limited to 25 MB on the wire. API admission may reject a message for domain,
rate, quota, suppression or workspace status. Temporary API failures return an
SMTP 4xx response so the client can retry. Retries of the same exact MIME and
envelope use a deterministic idempotency key.

Before public launch, test a valid key, a revoked key, an unverified sender,
unauthenticated relay rejection, attachment delivery, Bcc privacy, and a complete
bounce/complaint event. Exercise certificate renewal and database recovery. The
current deployment runs on one VPS and has brief interruptions during updates;
offer a public SLA only after adding redundant infrastructure and validating
capacity and abuse controls.
