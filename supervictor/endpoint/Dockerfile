# --- Build stage ---
# Keep the rustc here ≥ what the lockfile's MSRV demands — Dependabot bumps
# deps (libsqlite3-sys 0.38 needs cfg_select!, stable from 1.94) while this
# pin sits still, and CI never exercises the Docker path.
FROM rust:1.97-bookworm AS build

WORKDIR /app

# Copy common crate (shared dependency)
COPY supervictor/common ./supervictor/common

# Copy endpoint manifests for dependency caching
COPY supervictor/endpoint/Cargo.toml supervictor/endpoint/Cargo.lock ./supervictor/endpoint/
WORKDIR /app/supervictor/endpoint

# Dependency caching: build with dummy src, then replace. Cargo.toml declares
# an [[example]] (render_demo), and cargo refuses to parse the manifest unless
# the example file exists, so stub it too. Examples are never built here.
RUN mkdir src examples && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && echo "fn main() {}" > examples/render_demo.rs \
    && cargo build --release \
    && rm -rf src

COPY supervictor/endpoint/src ./src
RUN touch src/main.rs src/lib.rs \
    && cargo build --release

# --- Runtime stage ---
# The container path (docker-compose.yml: plain HTTP behind Caddy, which
# terminates mTLS and forwards the subject header). The default feature set
# builds here; the DynamoDB backend is `--features dynamo` if a deployment
# ever wants it.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/supervictor/endpoint/target/release/supervictor-endpoint /usr/local/bin/supervictor-endpoint

ENV PORT=8000
EXPOSE 8000
CMD ["supervictor-endpoint"]
