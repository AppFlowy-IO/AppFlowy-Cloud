FROM rust:1.56.1 as builder
WORKDIR /app

COPY . .
ENV SQLX_OFFLINE true
RUN RUSTFLAGS="-C opt-level=2" cargo build --release --bin appflowy_server
# Size optimization
#RUN strip ./target/release/appflowy_server

FROM debian:bullseye-slim AS runtime
WORKDIR /app
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/appflowy_server /usr/local/bin/appflowy_server
COPY --from=builder /app/configuration configuration
ENV APP_ENVIRONMENT production
CMD ["appflowy_server"]
