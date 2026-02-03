ARG RUST_VERSION=1.93

FROM rust:${RUST_VERSION}-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    protobuf-compiler \
    libprotobuf-dev \
    pkg-config \
    libssl-dev \
  && rm -rf /var/lib/apt/lists/*

ARG PACKAGE
ARG BIN

COPY . .
RUN cargo build --release -p "${PACKAGE}"

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
  && rm -rf /var/lib/apt/lists/*

ARG BIN
COPY --from=builder /app/target/release/${BIN} /usr/local/bin/app

ENV RUST_LOG=info
ENTRYPOINT ["/usr/local/bin/app"]
