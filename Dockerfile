FROM clux/muslrust:1.48.0-stable

WORKDIR /volume

COPY . .

RUN cargo build --release

FROM alpine:latest

RUN apk --no-cache add ca-certificates

COPY --from=0 /volume/target/x86_64-unknown-linux-musl/release/rust-trending /usr/local/bin

WORKDIR /app

CMD ["rust-trending"]
