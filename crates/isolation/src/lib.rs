//! Replaceable isolation boundary. No Docker SDK or host integration is exposed here.

use std::path::PathBuf;
use verify_sandbox_protocol::SandboxJobRequest;

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationConfig {
    pub read_only_root: bool,
    pub privileged: bool,
    pub docker_socket_mounted: bool,
    pub network_disabled: bool,
    pub workspace: PathBuf,
    pub pids_limit: u64,
    pub output_limit_bytes: u64,
}

impl IsolationConfig {
    #[must_use]
    pub fn safe_default(workspace: PathBuf) -> Self {
        Self {
            read_only_root: true,
            privileged: false,
            docker_socket_mounted: false,
            network_disabled: true,
            workspace,
            pids_limit: 256,
            output_limit_bytes: 10 * 1024 * 1024,
        }
    }
    /// # Errors
    ///
    /// Returns an error if any required isolation control is disabled or unbounded.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.read_only_root
            || self.privileged
            || self.docker_socket_mounted
            || !self.network_disabled
            || self.pids_limit == 0
            || self.output_limit_bytes == 0
        {
            return Err("unsafe isolation configuration");
        }
        Ok(())
    }
}

pub trait ExecutionBackend {
    /// Provisions an isolated execution configuration for a validated request.
    ///
    /// # Errors
    ///
    /// Returns an error when the request or resulting isolation configuration is unsafe.
    fn provision(&self, request: &SandboxJobRequest) -> Result<IsolationConfig, &'static str>;
}

pub struct DockerBackend;
impl ExecutionBackend for DockerBackend {
    fn provision(&self, request: &SandboxJobRequest) -> Result<IsolationConfig, &'static str> {
        request.validate().map_err(|_| "invalid request")?;
        let config = IsolationConfig::safe_default(PathBuf::from("/ephemeral/workspace"));
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verify_sandbox_protocol::{ArtifactPolicy, NetworkPolicy, ResourceLimits, SourceReference};

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
    fn safe_defaults_are_fail_closed() {
        let config = IsolationConfig::safe_default(PathBuf::from("/workspace"));
        assert!(config.validate().is_ok());
        assert!(!config.privileged);
        assert!(!config.docker_socket_mounted);
        assert!(config.network_disabled);
    }

    #[test]
    fn unsafe_configuration_is_rejected() {
        let mut config = IsolationConfig::safe_default(PathBuf::from("/workspace"));
        config.privileged = true;
        assert!(config.validate().is_err());
    }

    #[test]
    fn docker_backend_uses_safe_configuration() {
        let config = DockerBackend.provision(&request()).expect("safe request");
        assert!(config.validate().is_ok());
    }
}
