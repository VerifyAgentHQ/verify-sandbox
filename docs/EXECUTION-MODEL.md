# Execution model

Each job has one ephemeral sandbox lifecycle:

`RECEIVED → VALIDATING → ACCEPTED → PROVISIONING → RUNNING → COLLECTING → CLEANUP → RESULT`

This repository provides a process entrypoint that implements a JSON-lines transport: one JSON request per line on `stdin` and one JSON result per line on `stdout`. Diagnostics and logs are written only to `stderr` to preserve protocol purity. The process validates requests, invokes the runner/backend abstraction, and emits a single protocol response per request. The default process supports one request per process invocation (the `verify-agent` transport spawns the process per request).

Running may transition to `TIMED_OUT`, `CANCELLED`, `FAILED`, or `COMPLETED`; these map to the public `SandboxStatus` values. Validation rejects malformed requests and unsafe limits before provisioning. The Docker backend runs only JSON-encoded approved argv records for `pnpm` or `cargo`, applies read-only-root, non-privileged, no-network, memory, CPU, PID, timeout, and bounded-output controls, and destroys the container and host staging workspace in cleanup even after failure.

Source enters through a caller-configured `SourceMaterializer`; the directory materializer copies regular files into a fresh host staging directory and rejects symlinks. Docker receives that source through a bounded tar stream over `docker exec` into the `/workspace` tmpfs, never through `docker cp` or an arbitrary host bind mount. The read-only root remains enabled. Logs are represented by bounded opaque references; durable log storage is a later control-plane concern. The current public contract does not carry declared artifact paths, so declared artifacts fail closed.

The transfer uses POSIX PAX extended path records, so valid normalized repository-relative paths longer than the legacy 100-byte ustar name field are supported without truncation or aliasing.

The backend provides ephemeral tmpfs state paths for toolchain behavior: `/cargo-home` (512 MiB), `/pnpm-home` (512 MiB), and `/tmp` (256 MiB). `HOME`, Cargo, pnpm, npm-cache, and XDG cache variables are constructed explicitly; no host environment is inherited. These paths disappear with the container.

The development runner image is built locally or in CI before execution using
`scripts/build-runner-image.ps1` and verified with
`scripts/verify-runner-image.ps1`. Toolchains are present at image-build time;
execution does not download packages or toolchains.

The process reads the operator-only `VERIFY_SANDBOX_SNAPSHOT_ROOT` setting for
local/integration snapshots. The public `snapshot` field remains an opaque
identity, not a filesystem path. Materialization is read-only at the source and
is cleaned with the host staging directory after execution.

Requests should be idempotent by `jobId`: duplicate delivery must not reuse a workspace or cause an ambiguous second execution. Exactly-once execution requires control-plane coordination and is not claimed here. Cancellation is best effort and must still end in cleanup and a terminal public result.
