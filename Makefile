ROOT = "./build"
SEMVER_VERSION=$(shell grep version Cargo.toml | awk -F"\"" '{print $$2}' | head -n 1)

.PHONY: prerequisite local_test

prerequisite:
	POSTGRES_PORT=5432 ${ROOT}/init_database.sh
	${ROOT}/init_redis.sh

appflowy_cloud_image:
	source $(ROOT)/docker_env.sh && docker-compose up -d appflowy_cloud

local_test:
	# 🔥 Must run init_database first
	 SQLX_OFFLINE=true cargo test

run:
	${ROOT}/run_local_server.sh

test:
	${ROOT}/run_local_test.sh
