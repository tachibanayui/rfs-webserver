FROM lukemathwalker/cargo-chef:latest-rust-1-alpine3.22 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build and cache dependencies
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin rfs-webserver

# Runtime stage
FROM alpine:3.22.4

# RUN apk add --no-ca-certificates libgcc ca-certificates

WORKDIR /app
COPY --from=builder /app/target/release/rfs-webserver /app/rfs-webserver

EXPOSE 3000

CMD ["/app/rfs-webserver"]