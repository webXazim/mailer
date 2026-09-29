# CS Mailer site operator

This page manages **CS Mailer itself**: global SMTP pause/resume and daily cap, plus per-workspace sending pause/resume. Customer workspace `owner` and `admin` roles do not grant access.

The page is published only as `127.0.0.1:18085` on the VPS. The public Nginx listener returns 404 for `/operator/` and `/api/operator/`. The private listener requires the separate `operator` password and rejects cross-origin changes. Actions are written to the existing delivery or workspace audit tables.

## Set up on the VPS

From `/srv/apps/mailer` (use the actual Mailer checkout path):

```sh
git pull --ff-only origin main
sh manage production-env-upgrade
sh manage operator-init
sh manage deploy
sh manage healthcheck
```

`operator-init` prints a newly generated password **once**. Store the username `operator` and that password in a password manager. It will refuse to replace an existing password. The `.env` stays mode 0600. Deployment preflight requires a password of at least 32 characters.

On your own computer, open an SSH tunnel and leave its terminal running:

```sh
ssh -N -L 18085:127.0.0.1:18085 deploy@YOUR_VPS_IP
```

Open `http://localhost:18085/operator/`. Your browser asks for the `operator` credentials. Do not use the public `mailer.crescentsphere.com` hostname for this page.

Verify the binding on the VPS:

```sh
sudo ss -lntp '( sport = :18085 )'
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:18085/operator/
curl -s -o /dev/null -w '%{http_code}\n' https://mailer.crescentsphere.com/operator/
```

Expected results: `127.0.0.1:18085`, HTTP 401 without credentials, and public HTTP 404.
