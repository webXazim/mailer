#!/bin/sh
set -eu

cd "$(dirname "$0")/../.."

target=.env.stalwart
test ! -e "$target" || {
    echo "$target already exists; refusing to overwrite it" >&2
    exit 1
}

command -v openssl >/dev/null 2>&1 || {
    echo "openssl is required to generate the recovery credential" >&2
    exit 1
}

password=$(openssl rand -base64 36 | tr -d '\r\n' | tr '/+' '_-')

cat >"$target" <<EOF
STALWART_IMAGE=stalwartlabs/stalwart:v0.16
STALWART_ADMIN_PORT=8088
STALWART_PUBLIC_URL=https://smtp.crescentsphere.com
STALWART_RECOVERY_ADMIN=admin:$password
STALWART_HOSTNAME=smtp.crescentsphere.com
STALWART_IPV4=152.53.178.165
EOF

chmod 600 "$target"
echo "Created $target with a generated recovery credential."
echo "Keep it private. Remove STALWART_RECOVERY_ADMIN after permanent admin access is tested."

