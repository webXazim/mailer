# Production environment setup

Existing installations must merge variables added by later upgrades before
deploying a new build:

```sh
sudo sh manage production-env-upgrade
sudo vi .env
```

The upgrade command appends only missing names from `.env.production.example`. It
does not overwrite any existing value or generate provider credentials. Keep the
file mode 600 and never commit it.

## Hybrid SES and independent SMTP

Keep both provider credential sets in the application `.env` while SES rollback is
available. Use these non-secret routing values for Stalwart as the default:

```dotenv
DELIVERY_PROVIDER=smtp
DOMAIN_PROVIDER=stalwart
SMTP_HOST=smtp.crescentsphere.com
SMTP_PORT=465
SMTP_SECURITY=implicit_tls
SMTP_HELO_NAME=smtp.crescentsphere.com
SMTP_TIMEOUT_SECONDS=30
STALWART_API_URL=http://stalwart:8080
MTA_PUBLIC_HOST=smtp.crescentsphere.com
MTA_PUBLIC_IPV4=152.53.178.165
MTA_RETURN_PATH_PREFIX=bounce
```

Fill `SMTP_USERNAME` and `SMTP_PASSWORD` with the dedicated submission account
created in Stalwart. Fill `STALWART_API_TOKEN` with a restricted domain/DKIM API
token. Generate separate `STALWART_WEBHOOK_TOKEN` and
`STALWART_WEBHOOK_SIGNING_KEY` values and configure the same values in Stalwart's
delivery webhook. Do not use the recovery administrator credential for the API or
SMTP submission.

`SMTP_HOST` uses the public certificate name for TLS verification. The Stalwart
Compose stack assigns that name as an alias on the private mail network, so API
and worker containers connect directly to Stalwart without public hairpin routing.

Retain the `WORKER_AWS_*`, `SES_CONFIGURATION_SET`, `SES_EVENTS_QUEUE_URL`, and
`SES_EVENTS_TOPIC_ARN` values until SES rollback is deliberately retired.
`API_AWS_*` remains necessary only while `DOMAIN_PROVIDER=ses`; it may be empty
after domain management moves to Stalwart.

`DELIVERY_PROVIDER` controls the default for newly accepted production messages.
Existing per-workspace routes can switch immediately without editing `.env`:

```sh
sudo sh manage route-workspace WORKSPACE_UUID smtp
sudo sh manage route-workspace WORKSPACE_UUID ses
sudo sh manage route-workspace WORKSPACE_UUID default
```

Changing an environment selector requires redeploying the API and worker. A stored
message keeps its selected provider, and failover is allowed only before a provider
attempt begins.

## Stalwart and Garage files

Create these on the production VPS, beside the application `.env`:

```sh
sudo sh manage stalwart-init
sudo vi .env.stalwart
sudo sh manage stalwart-preflight
sudo sh manage stalwart-up

sudo sh manage storage-init
sudo vi .env.storage
sudo sh manage storage-preflight
sudo sh manage storage-up
sudo sh manage storage-bootstrap
sudo cat .storage/mailer.env
```

Copy the generated `OBJECT_STORAGE_*` entries from `.storage/mailer.env` into
`.env`. Use `OBJECT_STORAGE_PROVIDER=s3`. Garage credentials are independent from
Stalwart and AWS credentials.

Finish with:

```sh
sudo chmod 600 .env .env.stalwart .env.storage
sudo sh manage preflight
sudo sh manage deploy
sudo sh manage delivery-routing-status
```

Preflight intentionally fails while required secrets are blank. That prevents a
deployment from silently falling back to simulated or partially configured mail.
