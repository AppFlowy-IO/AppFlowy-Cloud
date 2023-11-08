FROM lukemathwalker/cargo-chef:latest-rust-1.69.0 as chef

WORKDIR /app
RUN apt update && apt install lld clang -y

FROM chef as planner
COPY . .
# Compute a lock-like file for our project
RUN cargo chef prepare --recipe-path recipe.json

FROM chef as builder
COPY --from=planner /app/recipe.json recipe.json
# Build our project dependencies
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
ENV SQLX_OFFLINE true

# install protobuf
RUN apt-get update -y && apt-get install -y protobuf-compiler

# Build the project
RUN cargo build --release --bin appflowy_cloud

FROM debian:bullseye-slim AS runtime
WORKDIR /app
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/appflowy_cloud /usr/local/bin/appflowy_cloud
COPY --from=builder /app/configuration configuration
ENV APP_ENVIRONMENT production
ENV RUST_BACKTRACE 1
CMD ["appflowy_cloud"]
