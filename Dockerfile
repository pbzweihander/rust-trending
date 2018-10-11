FROM alpine:latest

RUN apk --no-cache add ca-certificates

WORKDIR /app
COPY bin/rust-trending .

CMD ["./rust-trending"]
