ROOT = "./script"
SEMVER_VERSION=$(shell grep version Cargo.toml | awk -F"\"" '{print $$2}' | head -n 1)

.PHONY: run test

run:
	${ROOT}/run_local_server.sh

test:
	${ROOT}/run_local_test.sh
