FROM clux/muslrust

WORKDIR /volume

COPY . .

RUN cargo build --release

FROM alpine:latest

RUN apk --no-cache add ca-certificates

WORKDIR /app
COPY --from=0 /volume/target/x86_64-unknown-linux-musl/release/rust-trending .

CMD ["./rust-trending"]
