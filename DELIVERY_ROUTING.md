# Delivery routing and rollback

Mailer chooses and stores a delivery provider when a production email is accepted.
The worker never changes provider after a provider attempt exists. This prevents a
timeout or lost response from causing the same message to be submitted through a
second provider.

## Safe initial state

Migration `0021_delivery_routing_controls.sql` starts with SMTP paused, a 100-email
daily SMTP cap, and pre-attempt SES rollback enabled. `DELIVERY_PROVIDER` remains
the default for workspaces without an explicit route. Configure both provider
credential sets while operating a mixed cohort. Daily counters use the PostgreSQL
server date, which should remain UTC in production.

Inspect the controls before routing traffic:

```sh
sudo sh manage delivery-routing-status
```

Set an internal workspace as the first Stalwart cohort, set a conservative cap,
then resume SMTP:

```sh
sudo sh manage route-workspace WORKSPACE_UUID smtp
sudo sh manage smtp-cap 25
sudo sh manage smtp-resume
```

Use `default` to remove a workspace override. The workspace then follows
`DELIVERY_PROVIDER`:

```sh
sudo sh manage route-workspace WORKSPACE_UUID default
```

## Pause and rollback

```sh
sudo sh manage smtp-pause
sudo sh manage ses-rollback enable
```

The pause applies at the provider-attempt boundary. SMTP email that has no provider
attempt may be reassigned to SES when rollback is enabled and SES is fully
configured. With rollback disabled or unavailable, the worker leaves the email
queued and schedules another check. Submitted, permanently rejected, and
ambiguous attempts are never failed over.

Disabling rollback makes the pause a hard stop:

```sh
sudo sh manage ses-rollback disable
```

## Reputation review

Review delivery outcomes by provider and recipient domain:

```sh
sudo sh manage delivery-report 7
```

Increase the daily cap only after checking Stalwart queue age, temporary failures,
permanent bounces, complaints, and inbox placement. Recipient-domain concurrency
and retry timing remain Stalwart queue policy; remote SMTP `4xx` responses must
stay deferred in Stalwart rather than being resubmitted by Mailer.
