# CrescentSphere Mailer

Standalone transactional email platform for developers. Hosted mailbox
services are intentionally outside this repository.

## Projects

- [`frontend/`](frontend/README.md): React, Vite, and TypeScript console.
- [`backend/`](backend/README.md): Rust/Axum API, worker, PostgreSQL, NATS
  JetStream, SES, and R2 integration.

Run all normal development, verification, Docker, and deployment commands from
this directory through `sh manage`. The `Makefile` provides optional aliases.

## Development

```bash
test -e .env || cp .env.example .env
sh manage install
sh manage frontend-dev
```

Start the complete Docker development stack:

```bash
sh manage dev
```

Useful commands:

```bash
sh manage help
sh manage check
sh manage logs
sh manage down
sh manage compose-config
```

## VPS deployment (testing, then production)

Mailer runs beside Docgen and Messenger without using ports 80/443 or their
Docker networks/volumes. The standalone `docker-compose.production.yml` builds
all three application images on the VPS and includes its own Cloudflare Tunnel.
Do **not** combine it with `docker-compose.yml` (which remains for development).

Install Docker Engine with a current Docker Compose plugin, Git and OpenSSL.
No host Node.js, Rust, Nginx or Caddy installation is needed. Use a separate
checkout, for example `/opt/crescentsphere-mailer`.

First setup on the VPS:

```bash
cd /opt/crescentsphere-mailer
sh manage production-init
nano .env
chmod 600 .env
sh manage deploy
```

`production-init` creates `.env` with five independent random local secrets and
refuses to overwrite an existing file. Enter the tunnel token, separate API and
worker AWS credentials, SES/SQS event configuration, and R2 bucket credentials
at the **top** of the file. The lower section contains runtime defaults. If you
securely copy an existing `.env`, skip initialization and keep its secrets.
The ignored local `.env` is never delivered by Git; transfer it securely or enter
the credentials on the VPS. Never paste it into logs or tickets. Use shell-compatible
assignments; single-quote values containing `$`, spaces or `#`.

Additional values at the top of `.env`:

- `SES_CONFIGURATION_SET`: the exact SES configuration-set name whose event destination
  publishes to your SNS topic. Every production developer send selects it.
- `ACCOUNT_EMAIL_FROM`: a plain SES-verified sender address for account password resets,
  for example `accounts@your-verified-domain.com`. Verify it in SES before relying on recovery.
- `TURNSTILE_SITE_KEY` and `TURNSTILE_SECRET_KEY`: create a Cloudflare Turnstile
  widget for `mailer.crescentsphere.com`. The backend validates every public signup.

After deployment, create an account and verify its email. A workspace is created
automatically and starts in **Test**:
send from `sender@sandbox.mailer.invalid`, create a test key, and inspect the resulting
email/events. No domain or real recipient is needed for that simulation. Then add a
domain you control, publish its ownership TXT/DKIM/MAIL FROM records, verify it, and
ask the operator to approve the workspace, then use a **Production** key for real messages.
Operators run `sh manage pending-workspaces` followed by
`sh manage approve-workspace WORKSPACE_UUID`. SES sandbox accounts restrict recipients;
request SES production access before unrestricted sending.

In the Cloudflare dashboard, configure the supplied tunnel's published application:

| Setting | Value |
| --- | --- |
| Public hostname | `mailer.crescentsphere.com` |
| Service type | HTTP |
| Service URL | `api:8081` |
| Path | Leave empty (all paths) |

The `api` service owns the shared network namespace; port 8081 is **Nginx**, while
port 8080 is the private Rust API. The frontend shares that namespace so its
proxy reaches the API over loopback, as required by the backend's client-IP trust
checks. Cloudflare supplies `CF-Connecting-IP`; Nginx replaces `X-Real-IP` and
removes untrusted forwarding headers. Do not expose the origin or attach
untrusted containers to this network. Keep Cloudflare's visitor-IP header enabled.

Run only the Compose-managed cloudflared for this Mailer tunnel. Do not also run
the pasted standalone Docker command or change Docgen/Messenger tunnel routes.
No separate `api.mailer.crescentsphere.com` hostname is needed.

After any code change has reached the VPS, run:

```bash
sh manage deploy
```

If your VPS checkout follows a Git remote, update and rebuild in one command:

```bash
git pull --ff-only && sh manage deploy
```

The command validates configuration without printing secrets, builds before
replacing containers, waits for service health, and checks tunnel connectivity.
Build failures leave running containers untouched. Container replacement may
briefly interrupt service; this is not a zero-downtime or automatic-rollback setup.
Database migrations run automatically when the API starts. Back up before changes
that include migrations. Named PostgreSQL/NATS volumes survive redeploys and stops;
never use `down -v` or global Docker prune commands on this shared VPS.

Only a localhost console port is published. `FRONTEND_PORT=0` lets Docker choose
an available port, avoiding collisions with Docgen/Messenger. It may change when
the API container is recreated; the tunnel uses `api:8081` and needs no update.
Use `sh manage production-status` to see the assigned port. Optionally set a
verified free fixed port in `.env`. The API, database, NATS and tunnel metrics
have no public host bindings.

The public API base URL is `https://mailer.crescentsphere.com/api`; for example,
`POST /api/v1/emails`. `/api/internal/*` and `/internal/*` return 404. Console
health is `/healthz`; API checks are `/api/healthz` and `/api/readyz`.

```bash
sh manage production-status
sh manage production-logs            # or: sh manage production-logs api worker
sh manage healthcheck                # check DNS/routing after configuring Cloudflare
sh manage production-down            # stop only Mailer; keep data
sh manage production-pull            # refresh infrastructure images deliberately
```

The default runtime memory caps total about 1.8 GiB (ceilings, not reservations).
Leave additional RAM for Docker, PostgreSQL shared memory, the OS, Docgen and
Messenger. Rust builds need extra RAM and disk beyond runtime caps; build jobs
default to one and dependency/artifact caches are reused. Compile during quiet
periods and monitor free memory. On a constrained VPS, build on a separate host
instead of risking the other services. No memory capacity has been measured on
your VPS by this change.

### Testing and customer-launch checklist

Use `APP_ENV=production` even for VPS testing so HTTPS cookies and production
validation stay enabled. This requires real SES/SQS and R2 settings; test API keys
and test-environment email submissions simulate delivery. They do not exercise
real SES delivery. See [backend configuration](backend/README.md) for IAM,
SES event setup and a real delivery test. Cloudflare Tunnel carries HTTP traffic;
it is not an SMTP server or a replacement for SES.

The console now supports public registration with email verification and Cloudflare
Turnstile. New workspaces cannot send production email until operator approval. MFA, billing,
templates, and team management are not exposed. See [release notes](RELEASE_NOTES.md)
and the console's API guide for the supported contract.

Before customer use:

- Rotate the tunnel token shared in chat, update only the token in `.env`, and
  run `sh manage deploy`. Do not regenerate DB/NATS/webhook secrets on updates.
- Pin `CLOUDFLARED_IMAGE` to a reviewed version/digest instead of `latest`.
- Review SES production access, verified domains, IAM permissions and quotas.
- Configure encrypted offsite backups and rehearse recovery. See the backend
  recovery instructions; PostgreSQL backups alone do not back up NATS or R2.
- Run `sh manage healthcheck` and a complete real send/event/webhook test.

Reference: [Cloudflare tunnel run parameters](https://developers.cloudflare.com/tunnel/advanced/run-parameters/)
and [Docker Compose up / health waiting](https://docs.docker.com/reference/cli/docker/compose/up/).

### Deployment regression checks

On a Linux Docker host, `sh backend/deploy/tests/run.sh` checks secret generation,
configuration rejection, build-before-start sequencing, Nginx routing, blocked
internal paths and forwarded-header handling with a temporary API stub. It never
reads the real `.env` or starts a tunnel and removes its test containers afterward.
It builds a frontend test image and may download its test tool images; run it on
a development machine. This does not replace a full backend/provider smoke test.

`sh backend/deploy/tests/backend-smoke.sh` also builds the API and starts it with
disposable PostgreSQL/NATS volumes and dummy provider credentials. It checks
authenticated NATS startup, migrations, readiness and private-route blocking,
then deletes only its own test stack/data. It never starts cloudflared or the
sending worker. Real SES/R2 delivery still needs a separate test with your account.
