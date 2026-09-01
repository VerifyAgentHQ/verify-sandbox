# Architecture

```text
verify-agent
      ↓ SandboxJobRequest
   protocol (validation/normalization)
      ↓
   executor (lifecycle orchestration)
      ↓
   isolation backend (Docker now; microVM later)
      ↓
   untrusted code
      ↓ result collection
   SandboxJobResult
      ↓
verify-agent
```

The protocol layer maps the authoritative `verify-contracts` wire contract into validated internal request data and never executes commands. The execution layer owns lifecycle orchestration and timeout/cancellation behavior. The isolation layer owns workspace, process, network, and resource controls. Docker-specific concerns remain behind `ExecutionBackend`.

`DockerBackend` is the first concrete backend. It accepts only JSON-encoded structured argv records in the contract's command strings, resolves only the configured approved image, invokes Docker directly without a shell, streams a configured source tree into the `/workspace` tmpfs without a host bind mount, and removes the container and host staging directory in cleanup.

The approved development image is built from `infrastructure/runner/Dockerfile`
and tagged `verify-agent/runner:development`; image construction is separate
from runtime isolation and does not change backend controls.

Source materialization is provider-neutral: an operator-configured local
snapshot store maps an opaque snapshot ID to `<store-root>/<snapshot-id>`.
Requests never supply host paths. The materializer validates the ID, rejects
links and unsafe entries, applies bounded copy limits, and writes only to the
ephemeral destination prepared by the backend.

Operationally, the runner now performs an explicit validation step before provisioning any backend. Implementations must not provision resources for requests that fail protocol validation; this preserves a clear protocol → runner boundary and ensures fail-closed behavior.

The sandbox reports execution facts. Policy decisions and final verification status belong to VerifyAgent. The public contract currently has no artifact-path field, so `artifactPolicy=declared` is rejected until that boundary is explicitly extended; the contract is not modified here.
