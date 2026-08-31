# Execution model

Each job has one ephemeral sandbox lifecycle:

`RECEIVED → VALIDATING → ACCEPTED → PROVISIONING → RUNNING → COLLECTING → CLEANUP → RESULT`

Running may transition to `TIMED_OUT`, `CANCELLED`, `FAILED`, or `COMPLETED`; these map to the public `SandboxStatus` values. Validation rejects malformed requests and unsafe limits before provisioning. The executor runs only the explicit approved command list, applies hard timeout/resource controls, collects bounded references, and destroys the workspace in cleanup even after failure.

Requests should be idempotent by `jobId`: duplicate delivery must not reuse a workspace or cause an ambiguous second execution. Exactly-once execution requires control-plane coordination and is not claimed here. Cancellation is best effort and must still end in cleanup and a terminal public result.
