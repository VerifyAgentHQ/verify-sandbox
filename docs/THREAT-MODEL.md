# Threat model

## Assets

The host, VerifyAgent application data, user/project data, other jobs, and any GitHub, GOAT, AI, database, or cloud credentials are assets. Credentials are out of scope for the sandbox runtime and must never be injected.

## Threats and attack surfaces

Threat actors include malicious contributors, dependencies or registries, build/test scripts, compromised tools, and accidental destructive workloads. Source trees, package installation, explicit commands, filesystems, networks, artifacts, logs, and resource consumption are attack surfaces. Source and output may contain prompt injection; all such text is data, never instructions.

Controls are explicit execution plans, protocol validation, fail-closed policies, replaceable isolation, non-root/no-privilege execution, no socket or host mounts, read-only roots, ephemeral workspaces, disabled network by default, resource limits, bounded outputs, path checks, cleanup, and security-focused tests.

## Resource-exhaustion threats

Fork bombs, infinite loops, huge builds/logs, disk or memory exhaustion, process explosions, pathological dependency trees, and archive bombs require timeout, CPU, memory, PID, workspace, output, and artifact limits. This Phase 0 foundation models these controls and validates safe configuration; full enforcement belongs in the backend implementation.

## Residual risk

Docker configuration is not a production-grade multi-tenant boundary. Kernel escapes, daemon misconfiguration, supply-chain compromise, covert channels, and host-level denial of service remain possible. A hardened microVM or equivalent isolation layer requires separate security review before production use.
