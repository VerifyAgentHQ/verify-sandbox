# Security posture

This project is a security boundary for untrusted code. See [THREAT-MODEL.md](THREAT-MODEL.md) for assets, attacks, controls, and residual risk.

The Phase 0 foundation fails closed on invalid requests and unsafe isolation configuration. It defaults to no network, explicit commands, no inherited environment, non-root/non-privileged execution, no Docker socket, read-only root, bounded processes/output, and ephemeral workspaces. The current Docker adapter is a configuration boundary, not a production-ready arbitrary-code runner.

This repository enforces protocol-level validation and a fail-closed runner boundary. The `protocol` crate validates incoming `SandboxJobRequest` fields (schema version, per-command lengths, and resource limits) and `Runner::prepare()` validates requests before any backend provisioning. These checks are intended to reject malformed or unsafe requests early in the lifecycle.

Do not weaken controls to make local development convenient. Production use requires hardened isolation, host/daemon hardening, stronger resource enforcement, artifact security review, and an independent security review. Do not claim certification or multi-tenant guarantees.

Report suspected vulnerabilities privately to the repository maintainers; do not disclose exploitable details in a public issue until coordinated disclosure is agreed.

CI status: Phase 0 formatting, lint, and unit tests pass locally (`cargo fmt`, `cargo clippy`, `cargo test`).
