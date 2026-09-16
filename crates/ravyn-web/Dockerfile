FROM rust:1-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends build-essential curl \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown \
    && cargo install cargo-leptos --locked

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo leptos build --release -p ravyn-web

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/ravyn-web ./ravyn-web
COPY --from=builder /app/target/site ./site

ENV LEPTOS_SITE_ROOT=/app/site
ENV LEPTOS_SITE_ADDR=0.0.0.0:3001

EXPOSE 3001
ENTRYPOINT ["./ravyn-web"]
