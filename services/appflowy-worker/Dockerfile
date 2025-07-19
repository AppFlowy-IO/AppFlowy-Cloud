# Use the official Rust image as a builder
FROM rust:1.76 AS builder

# Set the working directory
WORKDIR /usr/src/appflowy-cloud

# Install build dependencies
RUN apt-get update && apt-get install -y protobuf-compiler

# Copy the Cargo files to cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY libs ./libs
COPY services ./services

# Create a dummy main.rs to cache dependencies
RUN mkdir -p src && echo "fn main() {}" > src/main.rs

# Build dependencies to cache them
RUN cargo build --release --locked --bin appflowy-worker

# Copy the rest of the application source code
COPY . .

# Build the application
RUN cargo build --release --locked --bin appflowy-worker

# Use a smaller, Debian-based image for the final image
FROM debian:buster-slim

# Set the working directory
WORKDIR /usr/local/bin

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/appflowy-cloud/target/release/appflowy-worker .

# Expose the application port
EXPOSE 8000

# Set the command to run the application
CMD ["appflowy-worker"]
