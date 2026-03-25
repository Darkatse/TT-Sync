# syntax=docker/dockerfile:1.7

FROM rust:1.94-alpine3.23 AS builder

RUN apk add --no-cache build-base cmake go musl-dev perl

WORKDIR /src

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --locked --release -p ttsync-cli \
    && strip target/release/tt-sync

FROM scratch

WORKDIR /app

COPY --from=builder /src/target/release/tt-sync /app/tt-sync

EXPOSE 8443
STOPSIGNAL SIGINT

ENTRYPOINT ["/app/tt-sync", "--state-dir", "/state", "--config-file", "/state/config.toml"]
CMD ["serve"]
