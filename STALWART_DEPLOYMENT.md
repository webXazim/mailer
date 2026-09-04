# Stalwart deployment

This is the first deployable step in the self-hosted transport roadmap. Stalwart
runs as a separate Compose project so Mailer application deploys cannot restart
or remove the SMTP queue.

## Before starting

The following must already be true:

- `smtp.crescentsphere.com` has a DNS-only A record for `152.53.178.165`.
- `152.53.178.165` has PTR `smtp.crescentsphere.com`.
- Netcup's default mail-block firewall policy has been deleted.
- Outbound IPv4 TCP 25 works.
- Host ports 25, 465, 587 and 8088 are unused.

Run:

```bash
sh manage stalwart-init
sh manage stalwart-network-check
sudo ss -ltnp | grep -E ':(25|465|587|8088)\b' || true
sh manage stalwart-up
sh manage stalwart-status
```

The stack uses IPv4-only Docker networking. Do not enable IPv6 until a selected
IPv6 address has matching AAAA, PTR and SPF authorization.

## Complete the setup wizard

The administration port is bound to VPS loopback. From a trusted workstation,
open a tunnel (replace the SSH host if needed):

```bash
ssh -L 8088:127.0.0.1:8088 deploy@152.53.178.165
```

Open <http://127.0.0.1:8088/admin>. The recovery username and password are in
`.env.stalwart` under `STALWART_RECOVERY_ADMIN`.

Use these initial values:

| Wizard field | Value |
| --- | --- |
| Server hostname | `smtp.crescentsphere.com` |
| Default email domain | `mailer.crescentsphere.com` |
| Automatic TLS | Enabled |
| Generate DKIM signing keys | Enabled |
| Storage | Local default for initial acceptance testing |
| Directory | Internal default |
| DNS management | Cloudflare DNS-01 with a narrowly scoped token, or manual |

Port 443 is intentionally not published because other applications may already
own it. Automatic TLS therefore requires DNS-01. A Cloudflare token used here
must be restricted to DNS editing for the required zone; do not reuse a global
API key or the Mailer OAuth client secret.

After the wizard, restart and inspect logs:

```bash
sh manage stalwart-restart
sh manage stalwart-logs
```

Create a permanent administrator and a dedicated SMTP submission principal for
the Mailer worker. Confirm permanent administrator access. Keep the recovery
credential during this initial upgrade because the bootstrap Compose definition
requires it and the admin listener is reachable only through VPS loopback. A later
hardening upgrade will remove the recovery environment entry and disable the
bootstrap HTTP listener together, after the permanent access path is proven.

## Acceptance checks

Do not route customer traffic yet. Complete all of these first:

- Unauthenticated relay to an unrelated domain is rejected.
- Authenticated submission over TLS succeeds.
- A Stalwart-specific DKIM selector is published without replacing SES DKIM.
- The direct-delivery return path has its own MX and SPF records.
- Gmail, Outlook and another receiver report SPF, DKIM and DMARC pass.
- A queued message survives a container restart.
- Configuration and data volumes are included in encrypted offsite backups.

Lifecycle commands:

```bash
sh manage stalwart-status
sh manage stalwart-logs
sh manage stalwart-restart
sh manage stalwart-down
```

`stalwart-down` removes containers and the project network but preserves named
volumes. Never add `-v` during normal operations.
