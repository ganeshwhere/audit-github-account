FROM rust:1.86-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates

RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/learning-rust /usr/local/bin/collab-dashboard
COPY --from=builder /app/templates ./templates

ENV RUST_LOG=info
EXPOSE 3000
CMD ["/usr/local/bin/collab-dashboard"]
