//! Boundary types and fail-closed validation for the authoritative sandbox protocol.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceReference {
    pub provider: String,
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLimits {
    pub timeout_ms: u64,
    pub memory_limit_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SandboxJobRequest {
    pub schema_version: String,
    pub job_id: String,
    pub source: SourceReference,
    pub snapshot: String,
    pub commands: Vec<String>,
    pub resource_limits: ResourceLimits,
    pub network_policy: NetworkPolicy,
    pub artifact_policy: ArtifactPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPolicy {
    None,
    Restricted,
    Allowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactPolicy {
    None,
    Declared,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RequestError {
    #[error("schema version must be 1.0.0")]
    UnsupportedVersion,
    #[error("request contains no approved commands")]
    EmptyCommands,
    #[error("resource limits must be positive")]
    InvalidLimits,
    #[error("command too long")]
    CommandTooLong,
    #[error("network and artifact policies must be explicit")]
    UnsafePolicy,
}

impl SandboxJobRequest {
    /// Validates the public request before any backend is provisioned.
    ///
    /// # Errors
    ///
    /// Returns an error when the version, command list, or resource limits are invalid.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.schema_version != "1.0.0" {
            return Err(RequestError::UnsupportedVersion);
        }
        if self.commands.is_empty() || self.commands.iter().any(String::is_empty) {
            return Err(RequestError::EmptyCommands);
        }
        // Enforce per-command size to align with the authoritative contract.
        if self.commands.iter().any(|c| c.len() > 4096) {
            return Err(RequestError::CommandTooLong);
        }
        // Basic resource limit checks: zero is invalid and memory must not exceed the contract's maximum.
        if self.resource_limits.timeout_ms == 0 || self.resource_limits.memory_limit_bytes == 0 {
            return Err(RequestError::InvalidLimits);
        }
        if self.resource_limits.memory_limit_bytes > 1_099_511_627_776u64 {
            return Err(RequestError::InvalidLimits);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn valid_request_is_accepted() {
        assert_eq!(request().validate(), Ok(()));
    }

    #[test]
    fn malformed_request_is_rejected() {
        let mut invalid = request();
        invalid.commands.clear();
        assert_eq!(invalid.validate(), Err(RequestError::EmptyCommands));
    }

    #[test]
    fn invalid_limits_are_rejected() {
        let mut invalid = request();
        invalid.resource_limits.timeout_ms = 0;
        assert_eq!(invalid.validate(), Err(RequestError::InvalidLimits));
    }

    #[test]
    fn long_command_is_rejected() {
        let mut invalid = request();
        invalid.commands[0] = "a".repeat(5000);
        assert_eq!(invalid.validate(), Err(RequestError::CommandTooLong));
    }
}
