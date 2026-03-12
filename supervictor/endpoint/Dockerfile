# --- Build stage ---
FROM rust:1.91-bookworm AS build

WORKDIR /app

# Copy common crate (shared dependency)
COPY supervictor/common ./supervictor/common

# Copy endpoint manifests for dependency caching
COPY supervictor/endpoint/Cargo.toml supervictor/endpoint/Cargo.lock ./supervictor/endpoint/
WORKDIR /app/supervictor/endpoint

# Dependency caching: build with dummy src, then replace
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
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
