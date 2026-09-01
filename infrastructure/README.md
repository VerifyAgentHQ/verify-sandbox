# Infrastructure

## Development runner image

`infrastructure/runner/Dockerfile` is the repository-owned build definition for
the exact backend image `verify-agent/runner:development`. It is a development
execution image, not the isolation boundary; `DockerBackend` remains responsible
for network, filesystem, privilege, resource, timeout, and cleanup controls.

Build and verify it from the repository root on Windows (PowerShell) or
PowerShell 7 on CI:

```powershell
./scripts/build-runner-image.ps1
./scripts/verify-runner-image.ps1
```

The Dockerfile pins Node `24.19.0`, pnpm `11.21.0`, and Rust/Cargo `1.98.0`,
based on the audited development toolchain because this repository currently
has no `rust-toolchain.toml` or package-manager metadata. Node and Rust are
sourced from their official Docker Library images; pnpm is installed from its
published npm package at the pinned version during the build. These upstream
tags and package contents remain a supply-chain risk until digest pinning and
artifact verification are added.

The final image uses a Node Debian bookworm-slim base, includes the Rust
toolchain copied from the pinned Rust build stage, runs as UID/GID `65532:65532`,
and uses `/workspace`. It has no entrypoint service, Docker CLI, Docker socket,
credentials, or runtime package-install requirement. The build never pushes.

At runtime, `DockerBackend` keeps the root filesystem read-only and supplies
bounded ephemeral tmpfs paths for `/workspace` (1 GiB), `/cargo-home` (512
MiB), `/pnpm-home` (512 MiB), and `/tmp` (256 MiB). It transfers source through
an explicit tar stream because `docker cp` is incompatible with this read-only
root configuration. Cargo and pnpm state therefore never uses host caches.

For local/integration execution, configure `VERIFY_SANDBOX_SNAPSHOT_ROOT` with
an operator-controlled snapshot-store directory. A request's opaque snapshot
ID selects only a validated child directory beneath that root; it is never
treated as a host path. Materialization is read-only, bounded to 1 GiB total,
100,000 files, 256 MiB per file, and 4096 bytes per path, and rejects symlinks,
junctions, reparse points, absolute paths, and traversal. Network-backed source
retrieval is intentionally not implemented.
