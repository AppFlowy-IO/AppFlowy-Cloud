ROOT = "./build"
SEMVER_VERSION=$(shell grep version Cargo.toml | awk -F"\"" '{print $$2}' | head -n 1)

.PHONY: init_database docker_image local_server docker_test

init_database:
	POSTGRES_PORT=5432 ${ROOT}/init_database.sh

docker_image:
	source $(ROOT)/docker_env.sh && docker-compose up -d postgres_db
	source $(ROOT)/docker_env.sh && docker-compose up -d appflowy_server

local_server:
	cargo run

docker_test:
	sh $(ROOT)/docker_test.sh

local_test:
	# 🔥 Must run init_database first
	 SQLX_OFFLINE=true cargo test


