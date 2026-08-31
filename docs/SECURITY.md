# Security posture

This project is a security boundary for untrusted code. See [THREAT-MODEL.md](THREAT-MODEL.md) for assets, attacks, controls, and residual risk.

The Docker backend fails closed on invalid requests and unsafe isolation configuration. It defaults to no network, explicit JSON argv commands, no inherited environment, non-root/non-privileged execution, no Docker socket, no host bind mounts, read-only root with a bounded temporary workspace, bounded processes/output, and ephemeral workspaces. Only preconfigured syntactically safe image references and `pnpm`/`cargo` executables are accepted; image pulls and arbitrary repository commands are not supported.

This repository enforces protocol-level validation and a fail-closed runner boundary. The `protocol` crate validates incoming `SandboxJobRequest` fields (schema version, per-command lengths, and resource limits) and `Runner::prepare()` validates requests before any backend provisioning. These checks are intended to reject malformed or unsafe requests early in the lifecycle.

Do not weaken controls to make local development convenient. Backend caps are one hour for timeout, 4 GiB for memory, 256 PIDs, 1 CPU, 10 MiB combined captured output, and 1 GiB source staging. `restricted` and `allowlist` networking are rejected because this backend cannot enforce them safely. `artifactPolicy=declared` is rejected because the approved request contract does not contain declared paths. Output is drained and bounded per stream, timeout/cancellation kills the Docker exec process and cleanup force-removes the container, and source symlinks/reparse points are rejected during materialization. Production use requires hardened isolation, host/daemon hardening, stronger resource enforcement, artifact security review, and an independent security review. Do not claim certification or multi-tenant guarantees.

Report suspected vulnerabilities privately to the repository maintainers; do not disclose exploitable details in a public issue until coordinated disclosure is agreed.

CI status: Phase 0 formatting, lint, and unit tests pass locally (`cargo fmt`, `cargo clippy`, `cargo test`).
