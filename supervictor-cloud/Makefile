.PHONY: deps build test test-integration validate deploy-dev deploy-prod local dev serve clean

# Export Lambda runtime dependencies from hello_world/pyproject.toml
deps:
	cd hello_world && uv export --no-dev --no-hashes -o requirements.txt

# Build SAM artifacts (runs deps first)
build: deps
	sam build

# Run unit tests
test:
	uv run pytest tests/unit/ -v

# Validate SAM template
validate:
	sam validate --lint

# Start local API (requires Docker)
local: build
	sam local start-api --env-vars .env.json

# Deploy to dev
deploy-dev: build
	sam deploy --config-env dev

# Deploy to prod (requires manual changeset approval)
deploy-prod: build
	sam deploy --config-env prod

# Run integration tests (requires Docker + SAM CLI)
test-integration:
	./scripts/run_integration_tests.sh

# Build, start sam local, run all tests
dev:
	./scripts/run_dev.sh

# Build and start sam local (leave running for manual testing)
serve:
	./scripts/run_dev.sh --serve

# Remove SAM build artifacts
clean:
	rm -rf .aws-sam
	rm -f hello_world/requirements.txt
