FROM lukemathwalker/cargo-chef:latest-rust-1.75.0 as chef

# Set the initial working directory
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

WORKDIR /app/services/collab-history
RUN cargo build --release --bin collab-history

FROM debian:bookworm-slim AS runtime
WORKDIR /app/services/collab-history
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app/
COPY --from=builder /app/target/release/collab-history /usr/local/bin/collab-history
ENV APP_ENVIRONMENT production
ENV RUST_BACKTRACE 1
CMD ["collab-history"]
