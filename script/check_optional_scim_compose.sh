#!/usr/bin/env bash

set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

# Keep template-only optional values from producing Compose warnings.
export APPFLOWY_S3_REGION="${APPFLOWY_S3_REGION:-us-east-1}"
export AZURE_OPENAI_API_KEY="${AZURE_OPENAI_API_KEY:-}"
export AZURE_OPENAI_ENDPOINT="${AZURE_OPENAI_ENDPOINT:-}"
export AZURE_OPENAI_API_VERSION="${AZURE_OPENAI_API_VERSION:-}"

base_compose=(
  docker compose
  --env-file deploy.env
  -f docker-compose.yml
)
scim_compose=(
  "${base_compose[@]}"
  -f docker-compose.scim.yml
)

default_services="$("${base_compose[@]}" config --services | sort)"
scim_services="$("${scim_compose[@]}" config --services | sort)"

if [[ "$default_services" != "$scim_services" ]]; then
  echo "SCIM override must not add or remove services" >&2
  exit 1
fi

default_config="$("${base_compose[@]}" config)"
if grep -q "/etc/nginx/appflowy-optional" <<<"$default_config"; then
  echo "Default compose configuration unexpectedly enables an optional Nginx route" >&2
  exit 1
fi

scim_config="$("${scim_compose[@]}" config)"
if ! grep -q "/etc/nginx/appflowy-optional" <<<"$scim_config"; then
  echo "SCIM compose configuration does not mount the optional Nginx route" >&2
  exit 1
fi

nginx_image="$(
  "${base_compose[@]}" config --images \
    | awk '/(^|\/)nginx(:|$)/ { print; exit }'
)"
if [[ -z "$nginx_image" ]]; then
  echo "Unable to resolve the Nginx image from docker-compose.yml" >&2
  exit 1
fi

nginx_mounts=(
  --mount "type=bind,src=$project_root/nginx/nginx.conf,dst=/etc/nginx/nginx.conf,readonly"
  --mount "type=bind,src=$project_root/nginx/ssl,dst=/etc/nginx/ssl,readonly"
)

docker run --rm "${nginx_mounts[@]}" "$nginx_image" nginx -t
docker run --rm \
  "${nginx_mounts[@]}" \
  --mount "type=bind,src=$project_root/nginx/optional/scim,dst=/etc/nginx/appflowy-optional,readonly" \
  "$nginx_image" \
  nginx -t

enabled_nginx_config="$(
  docker run --rm \
    "${nginx_mounts[@]}" \
    --mount "type=bind,src=$project_root/nginx/optional/scim,dst=/etc/nginx/appflowy-optional,readonly" \
    "$nginx_image" \
    nginx -T 2>&1
)"
if ! grep -Fq "location ^~ /scim/" <<<"$enabled_nginx_config" \
  || ! grep -Fq 'proxy_pass $appflowy_cloud_backend;' <<<"$enabled_nginx_config"; then
  echo "Enabled Nginx configuration does not contain the SCIM upstream route" >&2
  exit 1
fi

test_container="appflowy-scim-nginx-check-$$"
cleanup() {
  docker rm --force "$test_container" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run --rm --detach \
  --name "$test_container" \
  --publish 127.0.0.1::80 \
  "${nginx_mounts[@]}" \
  --mount "type=bind,src=$project_root/nginx/optional/scim,dst=/etc/nginx/appflowy-optional,readonly" \
  "$nginx_image" >/dev/null

host_port="$(docker port "$test_container" 80/tcp | sed 's/.*://')"
query_marker="scim-query-must-not-appear@example.com"
http_status=""
for _ in {1..50}; do
  http_status="$(
    curl --silent --output /dev/null --write-out '%{http_code}' \
      "http://127.0.0.1:${host_port}/scim/v2/Users?filter=userName%20eq%20%22${query_marker}%22" \
      || true
  )"
  [[ "$http_status" != "000" ]] && break
  sleep 0.1
done

if [[ "$http_status" != "403" ]]; then
  echo "Plaintext SCIM request returned $http_status instead of 403" >&2
  exit 1
fi

nginx_logs="$(docker logs "$test_container" 2>&1)"
if grep -q "$query_marker" <<<"$nginx_logs"; then
  echo "SCIM query value leaked into Nginx logs" >&2
  exit 1
fi

cleanup
trap - EXIT

echo "Default and SCIM-enabled Compose/Nginx configurations are valid"
