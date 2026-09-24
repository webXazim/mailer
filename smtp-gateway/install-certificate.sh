#!/bin/sh
# Certbot deploy hook; also safe to run manually after initial issuance.
set -eu

lineage=${RENEWED_LINEAGE:-/etc/letsencrypt/live/smtp.mailer.crescentsphere.com}
if [ -n "${RENEWED_LINEAGE:-}" ]; then
    case " ${RENEWED_DOMAINS:-} " in
        *' smtp.mailer.crescentsphere.com '*) ;;
        *) exit 0 ;;
    esac
fi
root=${MAILER_ROOT:-/srv/apps/mailer}
target=$root/secrets/smtp
test "$(id -u)" -eq 0 || { echo 'Run as root.' >&2; exit 1; }
test -f "$lineage/fullchain.pem" && test -f "$lineage/privkey.pem" || {
    echo 'SMTP certificate files are missing.' >&2
    exit 1
}
openssl x509 -in "$lineage/fullchain.pem" -noout -checkhost smtp.mailer.crescentsphere.com >/dev/null || {
    echo 'Certificate does not cover smtp.mailer.crescentsphere.com.' >&2
    exit 1
}
certificate_key=$(openssl x509 -in "$lineage/fullchain.pem" -pubkey -noout | openssl pkey -pubin -outform DER | sha256sum)
private_key=$(openssl pkey -in "$lineage/privkey.pem" -pubout -outform DER | sha256sum)
test "$certificate_key" = "$private_key" || {
    echo 'Certificate and private key do not match.' >&2
    exit 1
}
# Preflight runs as the deploy user. Allow directory traversal so it can check
# the public certificate without granting access to the private key.
install -d -m 0751 -o root -g 10001 "$target"
install -m 0644 -o root -g 10001 "$lineage/fullchain.pem" "$target/fullchain.pem"
install -m 0640 -o root -g 10001 "$lineage/privkey.pem" "$target/privkey.pem"
echo "Installed SMTP certificate in $target"

if [ -f "$root/.env" ]; then
    cd "$root"
    if [ -n "$(docker compose --project-name crescentsphere-mailer --profile smtp --env-file .env -f docker-compose.production.yml ps -q smtp_gateway)" ]; then
        docker compose --project-name crescentsphere-mailer --profile smtp --env-file .env -f docker-compose.production.yml restart smtp_gateway
    fi
fi
