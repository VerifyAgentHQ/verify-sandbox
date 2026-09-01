# Security posture

This project is a security boundary for untrusted code. See [THREAT-MODEL.md](THREAT-MODEL.md) for assets, attacks, controls, and residual risk.

The Docker backend fails closed on invalid requests and unsafe isolation configuration. It defaults to no network, explicit JSON argv commands, no inherited environment, non-root/non-privileged execution, no Docker socket, no host bind mounts, read-only root with a bounded temporary workspace, bounded processes/output, and ephemeral workspaces. Only preconfigured syntactically safe image references and `pnpm`/`cargo` executables are accepted; image pulls and arbitrary repository commands are not supported.

The development image is repository-owned and built from
`infrastructure/runner/Dockerfile`; it is not pulled automatically. Its pinned
toolchain versions and verification procedure are documented in
`infrastructure/README.md`. Image provenance is not yet digest-pinned, so
upstream base-image and package supply-chain risk remains and requires review
before production use.

Docker `cp` writes the container rootfs directly and is incompatible with the
read-only-root configuration. Source therefore enters through a tar stream to
`docker exec` and is extracted into the explicitly writable `/workspace` tmpfs.
Toolchain state uses only `/cargo-home` (512 MiB), `/pnpm-home` (512 MiB), and
`/tmp` (256 MiB) tmpfs mounts. The root filesystem remains read-only and these
paths are container-local and discarded during cleanup.

The local materializer permits at most 1 GiB total source bytes, 100,000 files,
256 MiB per file, and 4096 bytes per relative path. It resolves only validated
snapshot IDs beneath the configured store root and rejects absolute paths,
traversal, symlinks, junctions, and other Windows reparse points. No Git,
HTTP, package, or other network retrieval is performed.

This repository enforces protocol-level validation and a fail-closed runner boundary. The `protocol` crate validates incoming `SandboxJobRequest` fields (schema version, per-command lengths, and resource limits) and `Runner::prepare()` validates requests before any backend provisioning. These checks are intended to reject malformed or unsafe requests early in the lifecycle.

Do not weaken controls to make local development convenient. Backend caps are one hour for timeout, 4 GiB for memory, 256 PIDs, 1 CPU, 10 MiB combined captured output, and 1 GiB source staging. `restricted` and `allowlist` networking are rejected because this backend cannot enforce them safely. `artifactPolicy=declared` is rejected because the approved request contract does not contain declared paths. Output is drained and bounded per stream, timeout/cancellation kills the Docker exec process and cleanup force-removes the container, and source symlinks/reparse points are rejected during materialization. Production use requires hardened isolation, host/daemon hardening, stronger resource enforcement, artifact security review, and an independent security review. Do not claim certification or multi-tenant guarantees.

Report suspected vulnerabilities privately to the repository maintainers; do not disclose exploitable details in a public issue until coordinated disclosure is agreed.

CI status: Phase 0 formatting, lint, and unit tests pass locally (`cargo fmt`, `cargo clippy`, `cargo test`).
