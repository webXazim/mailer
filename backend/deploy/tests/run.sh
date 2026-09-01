#!/bin/sh
# Local deployment regression checks; never starts the real tunnel or providers.
set -eu
cd "$(dirname "$0")/../../.."
root=$(pwd)
api_id=
frontend_id=
cleanup() {
    if [ -n "$frontend_id" ]; then docker rm -f "$frontend_id" >/dev/null; fi
    if [ -n "$api_id" ]; then docker rm -f "$api_id" >/dev/null; fi
}
trap cleanup EXIT HUP INT TERM

docker run --rm --entrypoint sh --tmpfs /test:rw,exec \
    --mount "type=bind,source=$root/backend/deploy,target=/source/backend/deploy,readonly" \
    --mount "type=bind,source=$root/manage,target=/source/manage,readonly" \
    --mount "type=bind,source=$root/.env.production.example,target=/source/.env.production.example,readonly" \
    rust:1.97-bookworm /source/backend/deploy/tests/environment.sh

docker build --build-arg VITE_API_URL=/api --build-arg NGINX_CONFIG=nginx.production.conf \
    -t mailer-proxy-test:local frontend
api_id=$(docker run -d --publish 127.0.0.1:0:8081 \
    --mount "type=bind,source=$root/backend/deploy/tests,target=/tests,readonly" \
    node:22-alpine node /tests/proxy.mjs)
frontend_id=$(docker run -d --network "container:$api_id" \
    --read-only --cap-drop ALL --security-opt no-new-privileges \
    --tmpfs /var/cache/nginx:rw,noexec,nosuid,size=32m,uid=101,gid=101,mode=700 \
    --tmpfs /var/run:rw,noexec,nosuid,size=1m,uid=101,gid=101,mode=700 \
    mailer-proxy-test:local)
attempt=0
until docker exec "$frontend_id" wget -qO /dev/null http://127.0.0.1:8081/healthz; do
    attempt=$((attempt + 1))
    test "$attempt" -lt 10 || exit 1
    sleep 1
done
docker exec "$api_id" node /tests/proxy-check.mjs
docker port "$api_id" 8081
