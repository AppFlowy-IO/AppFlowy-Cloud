FROM rust:1.56.1 as builder
WORKDIR /app

COPY . .
WORKDIR /app/services/http_server
ENV SQLX_OFFLINE true
RUN RUSTFLAGS="-C opt-level=2" cargo build --release --bin http_server
# Size optimization
#RUN strip ./target/release/http_server

FROM debian:bullseye-slim AS runtime
WORKDIR /app
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/services/target/release/http_server /usr/local/bin/http_server
COPY --from=builder /app/services/http_server/configuration configuration
ENV APP_ENVIRONMENT production
CMD ["http_server"]
