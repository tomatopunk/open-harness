.PHONY: help fmt fmt-check lint test check build run-gateway run-manage run-channel run-orchestrator smoke docker-up docker-down clean

help:
	@echo "open-harness common commands"
	@echo ""
	@echo "  make fmt              - format all Rust code"
	@echo "  make fmt-check        - check formatting only"
	@echo "  make lint             - run clippy with warnings as errors"
	@echo "  make test             - run workspace tests"
	@echo "  make check            - run fmt-check + lint + test"
	@echo "  make build            - build workspace"
	@echo "  make smoke            - run curl smoke script"
	@echo "  make run-gateway      - run gateway service"
	@echo "  make run-manage       - run manage service"
	@echo "  make run-channel      - run channel service"
	@echo "  make run-orchestrator - run orchestrator service"
	@echo "  make docker-up        - docker compose up --build"
	@echo "  make docker-down      - docker compose down"
	@echo "  make clean            - clean cargo artifacts"

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

check: fmt-check lint test

build:
	cargo build --workspace --all-targets

run-gateway:
	cargo run -p open-harness-gateway

run-manage:
	cargo run -p open-harness-manage

run-channel:
	cargo run -p open-harness-channel

run-orchestrator:
	cargo run -p open-harness-orchestrator

smoke:
	./scripts/smoke.sh

docker-up:
	docker compose -f deploy/docker/docker-compose.yml up --build

docker-down:
	docker compose -f deploy/docker/docker-compose.yml down

clean:
	cargo clean
