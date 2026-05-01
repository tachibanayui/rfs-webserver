# Build stage
FROM rust:1.95.0-alpine3.22 AS builder

WORKDIR /app

# Cache dependencies
RUN cargo init
COPY Cargo.toml Cargo.lock ./
RUN cargo fetch
# Compile deps
RUN cargo build --release
RUN rm -r src

# Build application
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM alpine:3.22.4

WORKDIR /app
COPY --from=builder /app/target/release/rfs-webserver /app/rfs-webserver

EXPOSE 3000

CMD ["/app/rfs-webserver"]
