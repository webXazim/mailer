# Production controls

CrescentSphere Mailer uses one independent delivery path: authenticated SMTP
submission to Stalwart. Run commands from the repository root on the VPS.

## Status

```bash
sh manage production-status
sh manage delivery-routing-status
sh manage delivery-report 7
sh manage stalwart-status
sh manage stalwart-network-check
sh manage healthcheck
```

`delivery-routing-status` reports whether SMTP admission is paused, the daily
cap, recent usage, and control audit events.

## Pause and resume delivery

```bash
sh manage smtp-pause
sh manage smtp-resume
```

Pausing prevents a queued message from starting its first SMTP attempt. A
message with an ambiguous attempt is not automatically retried because the MTA
may already have accepted it.

## Daily admission cap

```bash
sh manage smtp-cap 1000
```

The cap applies to production messages admitted to SMTP during the current UTC
day. Test-mode simulations do not consume it.

## Workspace containment

```bash
sh manage pause-workspace WORKSPACE_UUID
sh manage resume-workspace WORKSPACE_UUID
sh manage security-events 7
```

Pause a workspace when abuse, credential exposure, or abnormal volume is
suspected. Resume only after investigation and any required key rotation.

## Service lifecycle

```bash
sh manage deploy
sh manage production-logs
sh manage stalwart-restart
sh manage stalwart-logs
```

The Mailer and Stalwart Compose projects are intentionally separate. Restarting
Mailer does not restart the MTA, and restarting Stalwart does not alter Mailer's
PostgreSQL or NATS data.

## Incident checks

For delayed or failed mail, check in this order:

1. `sh manage healthcheck`
2. `sh manage stalwart-status`
3. `sh manage stalwart-network-check`
4. `sh manage production-logs`
5. `sh manage stalwart-logs`

Confirm the SMTP TLS certificate, forward and reverse DNS, outbound IPv4 path,
DKIM signing, SPF/DMARC alignment, queue state, and signed event delivery. Keep
SMTP paused during repairs if new attempts would create uncertainty.

## Backup and recovery

```bash
sh manage backup
sh manage restore-rehearsal
```

Keep encrypted offsite PostgreSQL backups and test restoration regularly. The
object-storage backup is controlled separately by the settings in `.env`.
