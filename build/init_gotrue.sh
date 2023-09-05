#!/usr/bin/env bash

cd "$(dirname "$0")"
set -e

git clone https://github.com/supabase/gotrue.git
cp gotrue.env.docker gotrue/.env.docker
cd gotrue

# avoid port conflict with host postgres
sed -i "s/'5432:5432'/'5433:5432'/" docker-compose-dev.yml

make dev &

while true; do
	curl localhost:9999/health && break
	echo "waiting for gotrue to be ready..."
	sleep 1
done
