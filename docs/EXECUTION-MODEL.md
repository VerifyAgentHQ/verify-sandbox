# Execution model

Each job has one ephemeral sandbox lifecycle:

`RECEIVED → VALIDATING → ACCEPTED → PROVISIONING → RUNNING → COLLECTING → CLEANUP → RESULT`

This repository provides a process entrypoint that implements a JSON-lines transport: one JSON request per line on `stdin` and one JSON result per line on `stdout`. Diagnostics and logs are written only to `stderr` to preserve protocol purity. The process validates requests, invokes the runner/backend abstraction, and emits a single protocol response per request. The default process supports one request per process invocation (the `verify-agent` transport spawns the process per request).

Running may transition to `TIMED_OUT`, `CANCELLED`, `FAILED`, or `COMPLETED`; these map to the public `SandboxStatus` values. Validation rejects malformed requests and unsafe limits before provisioning. The Docker backend runs only JSON-encoded approved argv records for `pnpm` or `cargo`, applies read-only-root, non-privileged, no-network, memory, CPU, PID, timeout, and bounded-output controls, and destroys the container and host staging workspace in cleanup even after failure.

Source enters through a caller-configured `SourceMaterializer`; the directory materializer copies regular files into a fresh host staging directory and rejects symlinks. Docker receives that source through `docker cp`, never through an arbitrary host bind mount. Logs are represented by bounded opaque references; durable log storage is a later control-plane concern. The current public contract does not carry declared artifact paths, so declared artifacts fail closed.

Requests should be idempotent by `jobId`: duplicate delivery must not reuse a workspace or cause an ambiguous second execution. Exactly-once execution requires control-plane coordination and is not claimed here. Cancellation is best effort and must still end in cleanup and a terminal public result.
