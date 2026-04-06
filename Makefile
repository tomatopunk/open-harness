.PHONY: help fmt fmt-check lint test doc-check command-ir-guard pre-commit check build dev create-local-fs smoke acceptance docker-up docker-down clean e2e

help:
	@echo "open-harness common commands"
	@echo ""
	@echo "  make dev              - start development mode with local_fs storage (config.dev.yaml)"
	@echo "  make create-local-fs  - create local_fs directory structure for development"
	@echo "  make fmt              - format all Rust code"
	@echo "  make fmt-check        - check formatting only"
	@echo "  make pre-commit       - required checks before commit (fmt-check + lint + test + doc-check + command-ir-guard)"
	@echo "  make doc-check        - inner-engine roadmap / baseline doc consistency"
	@echo "  make command-ir-guard - ensure Command IR single entry in runtime"
	@echo "  make lint             - run clippy with warnings as errors"
	@echo "  make test             - run workspace tests"
	@echo "  make e2e              - run end-to-end tests"
	@echo "  make check            - run fmt-check + lint + test + e2e"
	@echo "  make build            - build workspace"
	@echo "  make smoke            - run curl smoke script"
	@echo "  make acceptance       - run acceptance script"
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

pre-commit: fmt-check lint test command-ir-guard

e2e:
	cargo test --manifest-path e2e/Cargo.toml --tests

check: fmt-check lint test e2e command-ir-guard

build:
	cargo build --workspace --all-targets

smoke:
	./scripts/smoke.sh

acceptance:
	./scripts/acceptance.sh

docker-up:
	docker compose -f deploy/docker/docker-compose.yml up --build

docker-down:
	docker compose -f deploy/docker/docker-compose.yml down

create-local-fs:
	@mkdir -p .deer-flow/local-fs/{config,tasks,threads,uploads,artifacts,memory,skills}
	@echo "Created local_fs directory structure at .deer-flow/local-fs/"

dev: create-local-fs
	cp config.dev.yaml config.yaml
	cp extensions_config.example.json extensions_config.json
	RUST_LOG=debug cargo run --bin open-harness-kernel

clean:
	cargo clean
