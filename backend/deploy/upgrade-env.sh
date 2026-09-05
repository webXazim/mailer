#!/bin/sh
# Add newly introduced production variables without changing existing values.
set -eu
cd "$(dirname "$0")/../.."
umask 077

test -f .env || { echo 'Run sh manage production-init first.' >&2; exit 1; }
test -f .env.production.example || { echo '.env.production.example is missing.' >&2; exit 1; }

temporary=$(mktemp .env.upgrade.XXXXXX)
trap 'rm -f "$temporary"' EXIT HUP INT TERM
cp .env "$temporary"
added=0

while IFS= read -r line || test -n "$line"; do
    case "$line" in
        [A-Za-z_][A-Za-z0-9_]*=*)
            name=${line%%=*}
            if ! grep -q "^${name}=" "$temporary"; then
                printf '%s\n' "$line" >>"$temporary"
                printf 'Added %s\n' "$name"
                added=$((added + 1))
            fi
            ;;
    esac
done <.env.production.example

chmod 600 "$temporary"
mv "$temporary" .env
trap - EXIT HUP INT TERM
echo "Environment upgrade complete: $added missing variable(s) added; existing values were preserved."
