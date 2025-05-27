# Base image for chef stages, platform will be set by buildx
FROM lukemathwalker/cargo-chef:latest-rust-1.85-bookworm AS chef
WORKDIR /usr/src

ENV SCCACHE_VERSION=0.10.0 
ENV RUSTC_WRAPPER=/usr/local/bin/sccache
ENV TARGETARCH=amd64 

# Download, configure sccache based on TARGETARCH
# Ensure curl is available in the base image or install it
RUN apt-get update && apt-get install -y curl && \
    SCCACHE_ARCHIVE="" && \
    if [ "${TARGETARCH}" = "amd64" ]; then SCCACHE_ARCHIVE="sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz"; \
    elif [ "${TARGETARCH}" = "arm64" ]; then SCCACHE_ARCHIVE="sccache-v${SCCACHE_VERSION}-aarch64-unknown-linux-musl.tar.gz"; \
    else echo "Unsupported TARGETARCH: ${TARGETARCH}" && exit 1; fi && \
    curl -fsSL "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/${SCCACHE_ARCHIVE}" | tar -xzv --strip-components=1 -C /usr/local/bin "$(echo ${SCCACHE_ARCHIVE} | sed 's/\.tar\.gz$//')/sccache" && \
    chmod +x /usr/local/bin/sccache && \
    apt-get purge -y curl && apt-get autoremove -y && rm -rf /var/lib/apt/lists/*

FROM chef AS planner

# Copy all workspace crates and main Cargo files
COPY lancedb_ffi lancedb_ffi
COPY search search
COPY search_ffi_types search_ffi_types
COPY backends backends
COPY core core
COPY router router
COPY Cargo.toml ./
COPY Cargo.lock ./

# 只在工作空间根目录运行一次 cargo chef prepare
RUN cargo chef prepare --recipe-path recipe.json

# Builder stage
FROM chef AS builder

ARG GIT_SHA
ARG DOCKER_LABEL
ARG SCCACHE_GHA_ENABLED # sccache GHA specific variable

# Install protobuf-compiler for lance-encoding and other build dependencies
RUN apt-get update && apt-get install -y protobuf-compiler build-essential pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src

# 从 planner 阶段复制统一的 recipe.json
COPY --from=planner /usr/src/recipe.json recipe.json

# 在工作空间根目录运行一次 cargo chef cook，编译所有依赖
RUN cargo chef cook --release --features candle,http --no-default-features --recipe-path recipe.json && sccache -s

# 先处理 lancedb_ffi
COPY lancedb_ffi lancedb_ffi
COPY search_ffi_types search_ffi_types

# 进入 lancedb_ffi 目录编译静态库
WORKDIR /usr/src/lancedb_ffi
RUN cargo build --release && sccache -s

# 回到工作空间根目录
WORKDIR /usr/src

# 复制其余工作空间源代码
COPY search search
COPY backends backends
COPY core core
COPY router router
COPY Cargo.toml ./
COPY Cargo.lock ./

# 编译应用程序二进制文件
RUN cargo build --release --bin text-embeddings-router --features candle,http --no-default-features && sccache -s

# Final runtime base image, platform will be set by buildx
FROM debian:bookworm-slim AS base

ENV HUGGINGFACE_HUB_CACHE=/data \
    PORT=80 \
    RAYON_NUM_THREADS=8

# Install minimal runtime dependencies
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# HTTP image
FROM base AS http

COPY --from=builder /usr/src/target/release/text-embeddings-router /usr/local/bin/text-embeddings-router

ENTRYPOINT ["text-embeddings-router"]
CMD ["--json-output"]

# Amazon SageMaker compatible image (if still needed)
# FROM http AS sagemaker
# COPY --chmod=775 sagemaker-entrypoint.sh entrypoint.sh # Ensure this script exists
# ENTRYPOINT ["./entrypoint.sh"]

# Default image
# FROM http