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

`DockerBackend` is the first concrete backend. It accepts only JSON-encoded structured argv records in the contract's command strings, resolves only the configured approved image, invokes Docker directly without a shell, copies a configured source tree into an ephemeral container workspace, and removes the container and host staging directory in cleanup.

Operationally, the runner now performs an explicit validation step before provisioning any backend. Implementations must not provision resources for requests that fail protocol validation; this preserves a clear protocol → runner boundary and ensures fail-closed behavior.

The sandbox reports execution facts. Policy decisions and final verification status belong to VerifyAgent. The public contract currently has no artifact-path field, so `artifactPolicy=declared` is rejected until that boundary is explicitly extended; the contract is not modified here.
