FROM docker.io/rust:1.85-bookworm
WORKDIR /app
COPY Cargo.toml .
COPY Cargo.lock .
COPY src src
RUN cargo build --release --locked

FROM docker.io/debian:bookworm
COPY --from=build /app/target/release/rust-trending /usr/local/bin/
CMD ["rust-trending"]
