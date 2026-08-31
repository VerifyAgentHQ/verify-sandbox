# verify-sandbox

Secure execution service for running untrusted verification workloads in an isolated security boundary.

```text
                verify-agent
                     |
             SandboxJobRequest
                     v
               verify-sandbox
                     |
             isolated execution
                     |
                     v
             untrusted repository
                     |
                     v
             SandboxJobResult
                     |
                     v
                verify-agent
```

The sandbox executes an explicit, trusted verification plan and reports execution facts. It does not decide policy, call GitHub/GOAT/AI services, or receive application secrets. Public request/result semantics are owned by [`verify-contracts`](../verify-contracts/schemas/sandbox/).

Phase 0 separates `protocol`, `execution`, and `isolation`. The initial backend is a Docker-oriented adapter behind an `ExecutionBackend` abstraction; Docker is not production-grade arbitrary-code or multi-tenant isolation. A hardened microVM backend remains a future production direction.

## Development

Install Rust and run:

```text
cargo fmt --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Read [`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md), [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/EXECUTION-MODEL.md`](docs/EXECUTION-MODEL.md), and [`docs/SECURITY.md`](docs/SECURITY.md) before changing isolation code.
