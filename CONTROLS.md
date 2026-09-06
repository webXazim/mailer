# CrescentSphere Mailer operator controls

This is the production operator runbook for CrescentSphere Mailer. Run commands
from the repository root on the VPS:

```sh
cd /srv/apps/mailer
```

Commands in this guide change live production behavior unless a section says
otherwise. Inspect the current state first and keep logs open during provider
changes.

## The two provider settings

Mailer has two independent provider choices:

| Control | Purpose | Values | Change method |
| --- | --- | --- | --- |
| Delivery provider | Sends accepted production email | `ses`, `smtp` | Runtime commands below |
| Domain provider | Creates and checks domain/DKIM configuration | `ses`, `stalwart` | Edit `DOMAIN_PROVIDER` in `.env`, then deploy |

`smtp` is the delivery name used for the Stalwart transport. Changing delivery
from Stalwart to SES does **not** require changing `DOMAIN_PROVIDER`. Leave domain
provisioning alone unless you intentionally want future domain management to use
a different control plane.

The runtime delivery override is stored in PostgreSQL and takes priority over the
boot-time `DELIVERY_PROVIDER` value in `.env`. A normal delivery switch does not
require a rebuild or restart.

## Always inspect first

```sh
sudo sh manage production-status
sudo sh manage healthcheck
sudo sh manage delivery-routing-status
sudo sh manage delivery-report 7
```

`delivery-routing-status` shows the runtime default, whether SMTP is paused, its
daily cap, whether SES rollback is enabled, recent provider volume, workspace
overrides, and the latest control changes.

`delivery-report 7` groups outcomes by provider and recipient domain for the last
seven days. Change `7` to any positive number of days.

## Switch Stalwart/SMTP to SES

This is the normal safe switch for all newly accepted mail:

```sh
sudo sh manage default-provider ses
sudo sh manage delivery-routing-status
sudo sh manage healthcheck
```

The switch applies to messages accepted after the database transaction commits.
Existing messages remain pinned to their recorded provider. Keep Stalwart running
until its already-submitted queue is empty and its final delivery events arrive.

If Stalwart has an incident and you also want to stop new SMTP attempts:

```sh
sudo sh manage ses-rollback enable
sudo sh manage default-provider ses
sudo sh manage smtp-pause
sudo sh manage delivery-routing-status
```

With rollback enabled and SES fully configured, SMTP messages that have not made
a provider attempt may be reassigned to SES. A message is never submitted to a
second provider after an attempt exists because a lost response could otherwise
produce a duplicate. Already-submitted Stalwart messages must finish through
Stalwart or be investigated individually.

Do not stop Stalwart merely because the default is SES. Stop it only after its
queue is drained and Mailer no longer needs its delivery events:

```sh
sudo sh manage stalwart-status
sudo sh manage stalwart-logs
sudo sh manage stalwart-down
```

`stalwart-down` preserves named volumes. Never add `-v`.

## Switch SES to Stalwart/SMTP

Both SMTP submission and Stalwart event credentials must already exist in `.env`.
Before routing customer traffic, verify the MTA and start with a small cohort:

```sh
sudo sh manage stalwart-status
sudo sh manage stalwart-network-check
sudo sh manage healthcheck
sudo sh manage smtp-cap 25
sudo sh manage ses-rollback enable
sudo sh manage smtp-resume
sudo sh manage route-workspace WORKSPACE_UUID smtp
sudo sh manage delivery-routing-status
```

Send real mail from that workspace to Gmail, Outlook, and another provider. Check
inbox placement, SPF/DKIM/DMARC, delivery events, temporary failures, bounces, and
complaints:

```sh
sudo sh manage delivery-report 1
sudo sh manage production-logs worker api
sudo sh manage stalwart-logs
```

When the cohort is healthy, make SMTP the default for newly accepted mail:

```sh
sudo sh manage default-provider smtp
sudo sh manage delivery-routing-status
```

Increase the daily cap gradually after reviewing reputation and queue health:

```sh
sudo sh manage smtp-cap 100
```

Existing SES messages remain on SES. The system does not move an attempted SES
message to SMTP.

## Restore the configured default

Clear the database override and follow `DELIVERY_PROVIDER` from `.env` again:

```sh
sudo sh manage default-provider environment
```

Use this only when you know the current `.env` value. Confirm it without printing
the rest of the secret file:

```sh
grep '^DELIVERY_PROVIDER=' .env
```

## Route one workspace

Use a workspace override for staged rollout, isolation, or provider comparison:

```sh
sudo sh manage route-workspace WORKSPACE_UUID smtp
sudo sh manage route-workspace WORKSPACE_UUID ses
sudo sh manage route-workspace WORKSPACE_UUID default
```

`default` deletes the workspace override. The workspace then follows the runtime
default, or `DELIVERY_PROVIDER` when the runtime override is `environment`.

Account verification and password-reset messages use the normal Mailer API. They
follow the route of the workspace owning the live `ACCOUNT_EMAIL_API_KEY`.

## Pause, resume, rollback, and cap SMTP

```sh
sudo sh manage smtp-pause
sudo sh manage smtp-resume
sudo sh manage ses-rollback enable
sudo sh manage ses-rollback disable
sudo sh manage smtp-cap POSITIVE_NUMBER
```

The pause is checked immediately before a provider attempt. It does not retract a
message already accepted by Stalwart. With SES rollback disabled or unavailable,
unattempted SMTP messages remain queued and are checked again later. Disable
rollback when a pause must be a hard hold; keep it enabled during a staged SMTP
rollout while SES remains configured.

The cap limits production messages admitted to SMTP per PostgreSQL calendar day.
The production database should use UTC. Reaching the cap prevents more SMTP
admission; it is not a Stalwart queue-size setting.

## Workspace production access and containment

```sh
sudo sh manage pending-workspaces
sudo sh manage approve-workspace WORKSPACE_UUID
sudo sh manage pause-workspace WORKSPACE_UUID
sudo sh manage resume-workspace WORKSPACE_UUID
sudo sh manage security-events 7
```

Production access is normally unlocked by verified-domain flow. Use manual
approval only for an investigated workspace. Before resuming one paused for abuse
or credential leakage, revoke or rotate affected API keys, review suppressions
and traffic, and resolve the cause. `resume-workspace` requeues eligible messages
that never reached a provider attempt.

## Account email troubleshooting

```sh
sudo sh manage account-email-status user@example.com
```

This joins the account-email queue to the downstream Mailer message and latest
event. Configure the sender without brackets around the display name:

```dotenv
ACCOUNT_EMAIL_FROM='CrescentSphere Mailer <no-reply@mailer.crescentsphere.com>'
```

`sent` means a provider or SMTP server accepted the message. `delivered` means the
recipient's mail server accepted it. Inbox placement is decided afterward by the
recipient and cannot be guaranteed by either status.

## Service lifecycle and logs

Deploy after pulling reviewed code:

```sh
git pull --ff-only
sudo sh manage production-env-upgrade
sudo vi .env
sudo chmod 600 .env
sudo sh manage preflight
sudo sh manage deploy
sudo sh manage healthcheck
```

`production-env-upgrade` appends missing variables without overwriting existing
values. `deploy` builds before replacing containers and runs database migrations
when the API starts.

| Task | Command |
| --- | --- |
| Show application containers and local port | `sudo sh manage production-status` |
| Follow all application logs | `sudo sh manage production-logs` |
| Follow selected logs | `sudo sh manage production-logs api worker` |
| Stop the application stack | `sudo sh manage production-down` |
| Pull PostgreSQL, NATS, and Cloudflared images | `sudo sh manage production-pull` |
| Show Stalwart status | `sudo sh manage stalwart-status` |
| Follow Stalwart logs | `sudo sh manage stalwart-logs` |
| Restart only Stalwart | `sudo sh manage stalwart-restart` |
| Stop Stalwart and preserve data | `sudo sh manage stalwart-down` |
| Start or update Stalwart | `sudo sh manage stalwart-up` |

Stalwart is a separate Compose project. A normal Mailer deploy does not restart
or remove its SMTP queue.

## Domain provider changes

Change `DOMAIN_PROVIDER` only when intentionally moving domain and DKIM
provisioning between SES and Stalwart. This is separate from delivery routing.

For Stalwart domain management, configure `STALWART_API_URL`, a restricted
`STALWART_API_TOKEN`, `MTA_PUBLIC_HOST`, `MTA_PUBLIC_IPV4`, and
`MTA_RETURN_PATH_PREFIX`. For SES domain management, configure the API AWS
credentials. Then run:

```sh
sudo sh manage preflight
sudo sh manage deploy
```

Existing DNS records are not automatically removed by changing this selector.
Keep valid SES and Stalwart DKIM records during migration so existing domains and
in-flight messages continue to authenticate.

## Backups, storage, and recovery

```sh
sudo sh manage backup
sudo sh manage restore-rehearsal
```

Backups require the configured age recipient and rclone destination. A restore
rehearsal uses a disposable database and must never target the live database.

For the independent Garage object store:

```sh
sudo sh manage storage-status
sudo sh manage storage-logs
sudo sh manage storage-restart
sudo sh manage storage-down
sudo sh manage storage-up
```

Stopping application or storage Compose projects without `-v` preserves named
volumes. Never use `docker compose down -v`, broad Docker prune commands, or
manual database edits as routine controls.

## Incident quick reference

| Situation | First actions |
| --- | --- |
| Stalwart unhealthy | Enable SES rollback, set default to SES, pause SMTP, inspect Stalwart logs |
| SES unhealthy | Confirm Stalwart health, resume SMTP, set a conservative cap, set default to SMTP |
| One workspace is abusive | Pause that workspace, rotate keys, inspect security events |
| Mail shows `sent` too long | Check event pipeline and API, worker, and provider logs; do not resend blindly |
| New deployment is unhealthy | Keep the provider stable, inspect `production-logs`, and run `healthcheck` |
| SMTP TLS, PTR, or port problem | Keep or switch the default to SES and run `stalwart-network-check` |

Every runtime routing change is written to `delivery_control_audit` and appears in
`delivery-routing-status`. Capture its before and after output during incidents.

## Complete `manage` command index

Run `sh manage help` on the installed version for its authoritative command list.

| Command | Effect |
| --- | --- |
| `install` | Install frontend dependencies |
| `dev` | Build and start the local Docker stack |
| `down` | Stop the local Docker stack |
| `logs` | Follow local stack logs |
| `frontend-dev` | Start the Vite development server |
| `frontend-build` | Build the frontend production bundle |
| `backend-fmt` | Check Rust formatting |
| `backend-lint` | Run strict Rust Clippy checks |
| `backend-test` | Run Rust tests |
| `check` | Run frontend build plus all backend checks |
| `compose-config` | Validate local Compose configuration |
| `production-init` | Create `.env` with generated local secrets; never overwrite it |
| `production-env-upgrade` | Append missing production variables without changing values |
| `preflight` | Validate production configuration without printing secrets |
| `deploy` / `production-up` | Validate, build, start, and wait for production services |
| `production-pull` | Pull infrastructure images only |
| `production-down` | Stop application services while preserving named volumes |
| `production-logs [SERVICES]` | Follow all or selected production logs |
| `production-status` | Show production services and assigned loopback port |
| `pending-workspaces` | List workspaces without production access |
| `approve-workspace ID` | Enable production for one workspace UUID |
| `pause-workspace ID` | Pause one workspace at admission and provider-attempt boundaries |
| `resume-workspace ID` | Resume and requeue its eligible unattempted messages |
| `security-events [DAYS]` | Show security and containment audit events |
| `account-email-status EMAIL` | Trace account email into downstream delivery status |
| `delivery-routing-status` | Show routing controls, usage, overrides, and audit history |
| `default-provider PROVIDER` | Set `ses`, `smtp`, or `environment` for new default traffic |
| `smtp-pause` / `smtp-resume` | Stop or allow new SMTP attempts |
| `smtp-cap NUMBER` | Set the global daily SMTP admission cap |
| `ses-rollback MODE` | Enable or disable pre-attempt SMTP-to-SES rollback |
| `route-workspace ID PROVIDER` | Set `ses`, `smtp`, or `default` for one workspace |
| `delivery-report [DAYS]` | Group delivery outcomes by provider and recipient domain |
| `healthcheck` | Check public console, API, worker progress, queues, and private routes |
| `backup` | Create and upload an encrypted PostgreSQL backup |
| `restore-rehearsal` | Restore the latest backup into a disposable database |
| `stalwart-init` | Create `.env.stalwart` with a recovery credential |
| `stalwart-preflight` | Validate Stalwart configuration |
| `stalwart-network-check` | Verify forward DNS, PTR, and outbound IPv4 SMTP |
| `stalwart-up` | Pull and start the independent Stalwart service |
| `stalwart-down` | Stop Stalwart while preserving its named volumes |
| `stalwart-restart` | Restart only Stalwart |
| `stalwart-logs` | Follow Stalwart logs |
| `stalwart-status` | Show Stalwart status and loopback admin port |
| `storage-init` | Create private Garage configuration and secrets |
| `storage-preflight` | Validate Garage configuration |
| `storage-up` | Pull and start Garage |
| `storage-bootstrap` | Create its layout, bucket, and application key |
| `storage-down` | Stop Garage while preserving named volumes |
| `storage-restart` | Restart only Garage |
| `storage-logs` | Follow Garage logs |
| `storage-status` | Show Garage health and loopback S3 port |

For setup prerequisites and implementation detail, also read
[Delivery routing and rollback](DELIVERY_ROUTING.md),
[Production environment setup](PRODUCTION_ENVIRONMENT.md), and
[Stalwart deployment](STALWART_DEPLOYMENT.md).
