//! Execution orchestration without policy or provider integrations.

use verify_sandbox_isolation::{ExecutionBackend, IsolationConfig};
use verify_sandbox_protocol::SandboxJobRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Received,
    Validating,
    Accepted,
    Provisioning,
    Running,
    TimedOut,
    Cancelled,
    Failed,
    Completed,
    Collecting,
    Cleanup,
    Result,
}

pub struct Runner<B: ExecutionBackend> {
    backend: B,
}
impl<B: ExecutionBackend> Runner<B> {
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }
    /// Provisions the backend after protocol validation.
    ///
    /// # Errors
    ///
    /// Returns an error when the request or backend security configuration is unsafe.
    pub fn prepare(&self, request: &SandboxJobRequest) -> Result<IsolationConfig, &'static str> {
        // Validate public request up-front before provisioning any backend resources.
        request.validate().map_err(|_| "invalid request")?;
        self.backend.provision(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verify_sandbox_isolation::DockerBackend;
    use verify_sandbox_protocol::{
        ArtifactPolicy, NetworkPolicy, ResourceLimits, SandboxJobRequest, SourceReference,
    };

    fn request() -> SandboxJobRequest {
        SandboxJobRequest {
            schema_version: "1.0.0".into(),
            job_id: "job-1".into(),
            source: SourceReference {
                provider: "source".into(),
                reference: "repo".into(),
            },
            snapshot: "snapshot-1".into(),
            commands: vec!["check".into()],
            resource_limits: ResourceLimits {
                timeout_ms: 1000,
                memory_limit_bytes: 1024,
            },
            network_policy: NetworkPolicy::None,
            artifact_policy: ArtifactPolicy::None,
        }
    }

    #[test]
    fn prepare_validates_request() {
        let runner = Runner::new(DockerBackend);
        let mut bad = request();
        bad.resource_limits.timeout_ms = 0;
        assert!(runner.prepare(&bad).is_err());
    }
}
