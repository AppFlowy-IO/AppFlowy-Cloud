# syntax=docker/dockerfile:1
# User should build from parent directory

FROM lukemathwalker/cargo-chef:latest-rust-1.86 as chef

WORKDIR /app

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

ARG ADMIN_FRONTEND_PORT
ARG PORT
ENV PORT=${ADMIN_FRONTEND_PORT:-${PORT:-3000}}
EXPOSE $PORT

CMD ["admin_frontend"]
