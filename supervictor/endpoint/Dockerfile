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

# Dependency caching: build with dummy src, then replace. The manifest also
# declares the render_demo example (ui-gated, GitHub Pages only), and cargo
# refuses to parse a manifest whose example path is missing — stub it and keep
# the stub: the real file is never compiled here (no `ui` feature) and isn't
# shipped in the image.
RUN mkdir src examples && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && echo "fn main() {}" > examples/render_demo.rs \
    && cargo build --release --features sqlite,dynamo \
    && rm -rf src

COPY supervictor/endpoint/src ./src
RUN touch src/main.rs src/lib.rs \
    && cargo build --release --features sqlite,dynamo

# --- Runtime stage ---
FROM debian:bookworm-slim

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:0.8.4 \
     /lambda-adapter /opt/extensions/lambda-adapter

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/supervictor/endpoint/target/release/supervictor-endpoint /usr/local/bin/supervictor-endpoint

ENV PORT=8000
EXPOSE 8000
CMD ["supervictor-endpoint"]
