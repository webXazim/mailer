#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
umask 077
test -f .env || { echo 'Run sh manage production-init first.' >&2; exit 1; }
command -v openssl >/dev/null || { echo 'OpenSSL is required.' >&2; exit 1; }
if grep -Eq '^MAILER_OPERATOR_PASSWORD=.{32,}$' .env; then
    echo 'Operator password is already configured. It was not changed.' >&2
    exit 1
fi
password=$(openssl rand -hex 32)
temporary=$(mktemp .env.operator.XXXXXX)
trap 'rm -f "$temporary"' EXIT HUP INT TERM
cp .env "$temporary"
if grep -q '^MAILER_OPERATOR_PASSWORD=' "$temporary"; then
    sed -i "s/^MAILER_OPERATOR_PASSWORD=.*/MAILER_OPERATOR_PASSWORD=$password/" "$temporary"
else
    printf '\nMAILER_OPERATOR_PASSWORD=%s\n' "$password" >> "$temporary"
fi
chmod 600 "$temporary"
mv "$temporary" .env
trap - EXIT HUP INT TERM
printf 'CS Mailer site operator username: operator\nCS Mailer site operator password: %s\nSave this password in your password manager now.\n' "$password"
