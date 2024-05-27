#!/usr/bin/env bash

cd "$(dirname "$0")"
set -e

git clone https://github.com/supabase/gotrue.git
cp gotrue.env.docker gotrue/.env.docker
cd gotrue

make dev &

while true; do
	curl localhost:9999/health && break
	echo "waiting for gotrue to be ready..."
	sleep 1
done
