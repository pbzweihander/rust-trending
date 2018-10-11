FROM clux/muslrust

WORKDIR /volume
COPY . .

RUN cargo build --release
RUN mv target/x86_64-unknown-linux-musl/release/rust-trending ./

FROM alpine:latest

RUN apk --no-cache add ca-certificates

WORKDIR /app
COPY --from=0 /volume/rust-trending .

CMD ["./rust-trending"]
