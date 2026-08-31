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

Operationally, the runner now performs an explicit validation step before provisioning any backend. Implementations must not provision resources for requests that fail protocol validation; this preserves a clear protocol → runner boundary and ensures fail-closed behavior.

The sandbox reports execution facts. Policy decisions and final verification status belong to VerifyAgent.
