# syntax=docker/dockerfile:1

# Versions are pinned to the toolchain audited in this repository's development
# environment. The official Docker Library images provide the build sources.
ARG NODE_VERSION=24.19.0
ARG PNPM_VERSION=11.21.0
ARG RUST_VERSION=1.98.0

FROM rust:${RUST_VERSION}-bookworm AS rust-toolchain

FROM node:${NODE_VERSION}-bookworm-slim

ARG PNPM_VERSION

LABEL org.opencontainers.image.title="VerifyAgent sandbox runner" \
      org.opencontainers.image.description="Minimal approved pnpm and Cargo execution environment" \
      org.opencontainers.image.version="development" \
      org.opencontainers.image.node-version="24.19.0" \
      org.opencontainers.image.pnpm-version="11.21.0" \
      org.opencontainers.image.rust-version="1.98.0"

# Install the approved package manager at image-build time only. No package
# installation is performed by the sandbox container at runtime.
RUN npm install --global pnpm@${PNPM_VERSION} \
    && npm cache clean --force \
    && pnpm --version

# Install TypeScript and Vitest globally so fixture snapshots can use minimal
# wrapper scripts in node_modules/.bin/ instead of shipping full node_modules.
# These are the only additional runtimes the TypeScript truth fixtures require.
ARG TYPESCRIPT_VERSION=5.8.3
ARG VITEST_VERSION=2.1.9
RUN npm install --global typescript@${TYPESCRIPT_VERSION} vitest@${VITEST_VERSION} \
    && npm cache clean --force \
    && tsc --version \
    && vitest --version

# NODE_PATH enables vitest to resolve its own imports (e.g. "vitest/config")
# from the global installation when invoked via wrapper scripts.
ENV NODE_PATH=/usr/local/lib/node_modules

# Copy only the Rust toolchain needed by the approved cargo command. The final
# image intentionally does not contain the Rust image's build-layer packages.
COPY --from=rust-toolchain /usr/local/cargo /usr/local/cargo
COPY --from=rust-toolchain /usr/local/rustup /usr/local/rustup

ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update \
    && apt-get install --no-install-recommends --yes passwd \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 65532 runner \
    && useradd --uid 65532 --gid 65532 --create-home --home-dir /home/runner \
        --shell /usr/sbin/nologin runner \
    && mkdir --mode=0755 /workspace \
    && chown 65532:65532 /workspace /home/runner \
    && node --version \
    && pnpm --version \
    && cargo --version \
    && rustc --version

USER 65532:65532
WORKDIR /workspace

# DockerBackend supplies its own long-lived sleep command and approved argv.
ENTRYPOINT []
