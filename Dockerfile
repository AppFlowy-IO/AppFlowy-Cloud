FROM lukemathwalker/cargo-chef:latest-rust-1.75.0 as chef

WORKDIR /app
RUN apt update && apt install lld clang -y

FROM chef as planner
COPY . .
# Compute a lock-like file for our project
RUN cargo chef prepare --recipe-path recipe.json

FROM chef as builder

# Specify a default value for FEATURES; it could be an empty string if no features are enabled by default
ARG FEATURES=""

COPY --from=planner /app/recipe.json recipe.json
# Build our project dependencies
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
ENV SQLX_OFFLINE true

# Build the project
RUN echo "Building with features: ${FEATURES}"
RUN cargo build --profile=profiling --features "${FEATURES}" --bin appflowy_cloud


FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/profiling/appflowy_cloud /usr/local/bin/appflowy_cloud
ENV APP_ENVIRONMENT production
ENV RUST_BACKTRACE 1
CMD ["appflowy_cloud"]
