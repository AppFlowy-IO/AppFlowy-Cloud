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

ARG PROFILE="release"

COPY --from=planner /app/recipe.json recipe.json
# Build our project dependencies with optimized settings
ENV CARGO_BUILD_JOBS=4
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN echo "Building worker with profile: ${PROFILE} "
RUN if [ "$PROFILE" = "release" ]; then \
      cargo chef cook --release --recipe-path recipe.json; \
    else \
      cargo chef cook --recipe-path recipe.json; \
    fi

COPY . .

WORKDIR /app/services/appflowy-worker
RUN if [ "$PROFILE" = "release" ]; then \
      cargo build --release --bin appflowy_worker; \
    else \
      cargo build --bin appflowy_worker; \
    fi

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
# Copy the binary from the appropriate target directory
ARG PROFILE="release"
RUN echo "Building worker with profile: ${PROFILE} "
RUN if [ "$PROFILE" = "release" ]; then \
      echo "Using release worker binary"; \
    else \
      echo "Using debug worker binary"; \
    fi
COPY --from=builder /app/target/$PROFILE/appflowy_worker /usr/local/bin/appflowy_worker
ENV APP_ENVIRONMENT production
ENV RUST_BACKTRACE  1
CMD ["appflowy_worker"]
