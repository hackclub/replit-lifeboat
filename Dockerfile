# Start with a Rust base image
FROM rust:1.76 as builder

# Install protobuf compiler
RUN apt-get update && apt-get install -y protobuf-compiler

# Create a new empty shell project
RUN USER=root cargo new --bin replit-takeout
WORKDIR /replit-takeout

# Copy the Cargo manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# Build dependencies first to cache them
RUN cargo build --release
RUN rm src/*.rs

COPY ./src ./src

# Build for release
RUN rm ./target/release/deps/replit_takeout*
RUN cargo build --release

# Start a new stage with a newer base image
FROM debian:bookworm-slim

# Install necessary dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    gnupg \
    lsb-release \
    sudo \
    libssl-dev \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the release binary from the builder stage
COPY --from=builder /replit-takeout/target/release/replit-takeout /usr/local/bin/replit-takeout

ENV RUST_LOG="debug"

# Set the startup command
CMD ["sh", "-c", "/usr/local/bin/replit-takeout"]
