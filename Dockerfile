FROM docker.io/rust:1.85-bookworm AS builder
WORKDIR /app
COPY Cargo.toml .
COPY Cargo.lock .
COPY src src
RUN cargo build --release --locked

FROM docker.io/debian:bookworm
COPY --from=builder /app/target/release/rust-trending /usr/local/bin/
CMD ["rust-trending"]
