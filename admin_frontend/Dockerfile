FROM lukemathwalker/cargo-chef:latest-rust-1.72.1 as chef

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
# Build the project
RUN cargo build --release --bin admin_frontend

FROM debian AS runtime
WORKDIR /app
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends libc6 \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/admin_frontend /usr/local/bin/admin_frontend
COPY Cargo.toml /app/Cargo.toml
ENV RUST_BACKTRACE 1
CMD ["admin_frontend"]
