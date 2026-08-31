# Agent instructions

**The sandbox is a security boundary. Convenience is not a valid reason to weaken it.**

- Read the security documentation before modifying isolation code.
- Do not weaken isolation for convenience.
- Never add privileged execution or mount the Docker socket.
- Never pass application secrets or the host environment to workloads.
- Network must fail closed; `none` is the default.
- Execute only commands in an approved execution plan, never arbitrary repository-discovered commands.
- Do not add external dependencies without security and maintenance justification.
- Do not silently change the threat model.
- Security changes require tests and documentation.
- Preserve the authoritative contracts in `../verify-contracts`; do not redefine them here.
- Do not modify `verify-agent` or `verify-contracts`.
- Run formatting, clippy, checks, and tests before completing work.
- Do not bypass or weaken `protocol` validation or `Runner::prepare()`; failing requests must be rejected before provisioning.
