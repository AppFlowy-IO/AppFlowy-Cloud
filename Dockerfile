# syntax=docker/dockerfile:1
# Using cargo-chef to manage Rust build cache effectively
FROM lukemathwalker/cargo-chef:latest-rust-1.86 as chef

WORKDIR /app
RUN apt update && apt install lld clang -y

FROM chef as planner
COPY . .
# Compute a lock-like file for our project
RUN cargo chef prepare --recipe-path recipe.json

FROM chef as builder

# Update package lists and install protobuf-compiler along with other build dependencies
RUN apt update && apt install -y protobuf-compiler lld clang

# Specify a default value for FEATURES; it could be an empty string if no features are enabled by default
ARG FEATURES=""
ARG PROFILE="release"

COPY --from=planner /app/recipe.json recipe.json
ENV CARGO_BUILD_JOBS=4
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
# Fork-only: tame release-profile memory for self-hosted Docker builds.
# Upstream's `[profile.release]` ships `lto = true`, `opt-level = 3`,
# `codegen-units = 1`. That combination peaks at 6+ GiB to link the bin and
# 3+ GiB just to rustc-compile the `appflowy-cloud` lib crate (sqlx
# query macros + Actix typed router blow up the HIR). On a 4 GiB Colima
# this OOM-kills both phases.
#
# Setting LTO off + opt-level=2 caps memory at ~2 GiB total. Both values
# are reverted via Cargo's CARGO_PROFILE_RELEASE_* env override mechanism
# without touching Cargo.toml, so the upstream PR for the relation patch
# stays clean. Self-hosted instances are I/O-bound (Postgres / Redis / S3
# / gotrue round trips dominate runtime), so the perf delta is negligible.
ENV CARGO_PROFILE_RELEASE_LTO=off
ENV CARGO_PROFILE_RELEASE_OPT_LEVEL=2
# Reduce memory usage during compilation
RUN echo "Building appflowy cloud with profile: ${PROFILE}"
RUN if [ "$PROFILE" = "release" ]; then \
      cargo chef cook --release --recipe-path recipe.json; \
    else \
      cargo chef cook --recipe-path recipe.json; \
    fi

COPY . .
ENV SQLX_OFFLINE true

# Build the project
RUN echo "Building with profile: ${PROFILE}, features: ${FEATURES}, "
RUN if [ "$PROFILE" = "release" ]; then \
      cargo build --release --features "${FEATURES}" --bin appflowy_cloud; \
    else \
      cargo build --features "${FEATURES}" --bin appflowy_cloud; \
    fi

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update -y \
  && apt-get install -y --no-install-recommends openssl ca-certificates curl \
  && update-ca-certificates \
  # Clean up
  && apt-get autoremove -y \
  && apt-get clean -y \
  && rm -rf /var/lib/apt/lists/*

# Copy the binary from the appropriate target directory
ARG PROFILE="release"
RUN echo "Building with profile: ${PROFILE}"
RUN if [ "$PROFILE" = "release" ]; then \
      echo "Using release binary"; \
    else \
      echo "Using debug binary"; \
    fi
COPY --from=builder /app/target/$PROFILE/appflowy_cloud /usr/local/bin/appflowy_cloud
ENV APP_ENVIRONMENT production
ENV RUST_BACKTRACE 1

ARG APPFLOWY_APPLICATION_PORT
ARG PORT
ENV PORT=${APPFLOWY_APPLICATION_PORT:-${PORT:-8000}}
EXPOSE $PORT

CMD ["appflowy_cloud"]
