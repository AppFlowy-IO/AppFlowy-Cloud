# User should build from parent directory

FROM lukemathwalker/cargo-chef:latest-rust-1.77 as chef

WORKDIR /app
RUN apt update && apt install lld clang -y

# FROM chef as planner
# COPY . .
# # Compute a lock-like file for our project
# WORKDIR /app/admin_frontend
# RUN cargo chef prepare --recipe-path recipe.json
#
# FROM chef as builder
# COPY . .
# WORKDIR /app/admin_frontend
# COPY --from=planner /app/admin_frontend/recipe.json recipe.json
# # Build our project dependencies
# RUN cargo chef cook --release --recipe-path recipe.json
#
# # Build the project
# RUN cargo build --release --bin admin_frontend

# Chef failed to build, so we're using the following instead
FROM chef as builder
COPY . .
WORKDIR /app/admin_frontend
RUN cargo build --release --bin admin_frontend

FROM debian AS runtime
WORKDIR /app
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends libc6 libssl-dev \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/admin_frontend /usr/local/bin/admin_frontend
COPY --from=builder /app/admin_frontend/assets /app/assets
ENV RUST_BACKTRACE 1
ENV RUST_LOG info
CMD ["admin_frontend"]
