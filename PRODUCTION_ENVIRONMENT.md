# Production environment setup

Existing installations must merge variables added by later upgrades before
deploying a new build:

```sh
sudo sh manage production-env-upgrade
sudo vi .env
```

The upgrade command appends missing names from `.env.production.example`, removes
retired managed-provider variables, and does not overwrite other existing values
or generate provider credentials. Keep the
file mode 600 and never commit it.

## Independent SMTP

Use these non-secret values for Stalwart:

```dotenv
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

All production messages use this SMTP path. A provider attempt is recorded before
network I/O, and uncertain results require operator review to prevent duplicates.

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
Stalwart credentials.

Finish with:

```sh
sudo chmod 600 .env .env.stalwart .env.storage
sudo sh manage preflight
sudo sh manage deploy
sudo sh manage delivery-routing-status
```

Preflight intentionally fails while required secrets are blank. That prevents a
deployment from silently falling back to simulated or partially configured mail.
