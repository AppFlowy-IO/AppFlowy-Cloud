# syntax=docker/dockerfile:1
FROM lukemathwalker/cargo-chef:latest-rust-1.86 as chef

# Set the initial working directory
WORKDIR /app
RUN apt update && apt install lld clang -y

FROM chef as planner
COPY . .

# Compute a lock-like file for our project
RUN cargo chef prepare --recipe-path recipe.json

FROM chef as builder

# Update package lists and install protobuf-compiler along with other build dependencies
RUN apt update && apt install -y protobuf-compiler lld clang

COPY --from=planner /app/recipe.json recipe.json
# Build our project dependencies
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .

WORKDIR /app/services/appflowy-worker
RUN cargo build --release --bin appflowy_worker

FROM debian:bookworm-slim AS runtime
WORKDIR /app/services/appflowy-worker
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl \
    && apt-get install -y -y ca-certificates \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app/
COPY --from=builder /app/target/release/appflowy_worker /usr/local/bin/appflowy_worker
ENV APP_ENVIRONMENT production
ENV RUST_BACKTRACE  1
CMD ["appflowy_worker"]
