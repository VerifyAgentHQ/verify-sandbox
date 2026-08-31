//! Execution orchestration without policy or provider integrations.

use std::io::{Read, Write};

use verify_sandbox_isolation::{CancellationToken, ExecutionBackend, IsolationConfig};
use verify_sandbox_protocol::{SandboxJobRequest, SandboxJobResult};

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

    /// Executes after validation and provisioning have both succeeded.
    ///
    /// # Errors
    ///
    /// Returns an error when validation, provisioning, or backend execution fails.
    pub fn execute(
        &self,
        request: &SandboxJobRequest,
        cancellation: Option<&CancellationToken>,
    ) -> Result<SandboxJobResult, String> {
        self.prepare(request).map_err(str::to_owned)?;
        self.backend
            .execute(request, cancellation)
            .map_err(|error| error.to_string())
    }
}

/// Process a single JSON-lines request from `reader` and write a single JSON-lines
/// response to `stdout`. Diagnostics are written to `stderr`.
///
/// # Errors
/// Returns `Err(code)` when the request is invalid (malformed JSON, oversize)
/// or when a fatal I/O error occurs reading the request. Return codes are:
/// - `2`: input validation or read error
///
/// # Panics
/// May panic if serialization of the result fails unexpectedly; callers should
/// treat such panics as implementation bugs.
pub fn run_once<R: Read, W: Write, E: Write, B: ExecutionBackend>(
    reader: &mut R,
    mut stdout: W,
    mut stderr: E,
    backend: B,
    max_request_bytes: usize,
) -> Result<(), i32> {
    let mut buffer = Vec::new();
    let mut limited = reader.take(max_request_bytes as u64 + 1);
    match limited.read_to_end(&mut buffer) {
        Ok(size) => {
            if size > max_request_bytes {
                let _ = writeln!(stderr, "request exceeded maximum size");
                return Err(2);
            }
        }
        Err(err) => {
            let _ = writeln!(stderr, "failed reading request: {err}");
            return Err(2);
        }
    }

    // Trim trailing whitespace/newlines
    while buffer.last().is_some_and(u8::is_ascii_whitespace) {
        buffer.pop();
    }

    let Ok(raw) = String::from_utf8(buffer) else {
        let _ = writeln!(stderr, "request is not valid UTF-8");
        return Err(2);
    };

    let request: SandboxJobRequest = if let Ok(req) = serde_json::from_str(&raw) {
        req
    } else {
        let _ = writeln!(stderr, "invalid JSON request");
        return Err(2);
    };

    // Validate using protocol layer
    if let Err(err) = request.validate() {
        // Produce a protocol result indicating the validation error.
        let result = serde_json::json!({
            "schemaVersion": "1.0.0",
            "jobId": request.job_id,
            "status": "error",
            "durationMs": 0,
            "logsRef": "",
            "artifactRefs": [],
            "resourceUsage": { "memoryBytes": 0, "cpuTimeMs": 0 },
            "errors": [format!("validation error: {:?}", err)]
        });
        let _ = writeln!(stdout, "{}", serde_json::to_string(&result).unwrap());
        return Ok(());
    }

    // Execute via runner and backend
    let runner = Runner::new(backend);
    match runner.execute(&request, None) {
        Ok(result) => {
            let serialized = serde_json::to_string(&result).unwrap_or_else(|_| {
                serde_json::json!({
                    "schemaVersion": "1.0.0",
                    "jobId": request.job_id,
                    "status": "error",
                    "durationMs": 0,
                    "logsRef": "",
                    "artifactRefs": [],
                    "resourceUsage": { "memoryBytes": 0, "cpuTimeMs": 0 },
                    "errors": ["failed to serialize result"]
                })
                .to_string()
            });
            let _ = writeln!(stdout, "{serialized}");
            Ok(())
        }
        Err(err) => {
            // Convert runner/backend error into an error result per contract
            let result = serde_json::json!({
                "schemaVersion": "1.0.0",
                "jobId": request.job_id,
                "status": "error",
                "durationMs": 0,
                "logsRef": "",
                "artifactRefs": [],
                "resourceUsage": { "memoryBytes": 0, "cpuTimeMs": 0 },
                "errors": [err]
            });
            let _ = writeln!(stdout, "{}", serde_json::to_string(&result).unwrap());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use verify_sandbox_isolation::{BackendError, IsolationConfig};
    use verify_sandbox_protocol::{
        ArtifactPolicy, NetworkPolicy, ResourceLimits, ResourceUsage, SandboxJobRequest,
        SandboxJobResult, SandboxStatus, SourceReference,
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
        let runner = Runner::new(verify_sandbox_isolation::DockerBackend::default());
        let mut bad = request();
        bad.resource_limits.timeout_ms = 0;
        assert!(runner.prepare(&bad).is_err());
    }

    struct TestBackend;
    impl verify_sandbox_isolation::ExecutionBackend for TestBackend {
        fn provision(&self, _request: &SandboxJobRequest) -> Result<IsolationConfig, &'static str> {
            Ok(IsolationConfig::safe_default(std::path::PathBuf::from(
                "/tmp",
            )))
        }
        fn execute(
            &self,
            request: &SandboxJobRequest,
            _cancellation: Option<&verify_sandbox_isolation::CancellationToken>,
        ) -> Result<SandboxJobResult, BackendError> {
            Ok(SandboxJobResult {
                schema_version: "1.0.0".into(),
                job_id: request.job_id.clone(),
                status: SandboxStatus::Completed,
                exit_code: Some(0),
                duration_ms: 1,
                logs_ref: "fixture://logs".into(),
                artifact_refs: Vec::new(),
                resource_usage: ResourceUsage {
                    memory_bytes: 0,
                    cpu_time_ms: 0,
                },
                errors: Vec::new(),
            })
        }
    }

    #[test]
    fn process_valid_request_returns_result_on_stdout() {
        let req = request();
        let input = serde_json::to_string(&req).unwrap() + "\n";
        let mut stdin = Cursor::new(input.into_bytes());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let res = run_once(
            &mut stdin,
            &mut stdout,
            &mut stderr,
            TestBackend,
            1024 * 1024,
        );
        assert!(res.is_ok());
        let out = String::from_utf8(stdout).unwrap();
        let parsed: SandboxJobResult = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(parsed.job_id, req.job_id);
        assert_eq!(parsed.status, SandboxStatus::Completed);
        assert!(stderr.is_empty());
    }

    #[test]
    fn process_invalid_json_returns_error_exit() {
        let mut stdin = Cursor::new(b"not-json\n".to_vec());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let res = run_once(&mut stdin, &mut stdout, &mut stderr, TestBackend, 1024);
        assert!(res.is_err());
        assert!(!stderr.is_empty());
        assert!(stdout.is_empty());
    }

    #[test]
    fn process_oversized_request_is_rejected() {
        // create a request larger than the limit
        let req = request();
        let mut huge = serde_json::to_string(&req).unwrap();
        huge.push_str(&"x".repeat(2048));
        let mut stdin = Cursor::new(huge.into_bytes());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let res = run_once(&mut stdin, &mut stdout, &mut stderr, TestBackend, 1024);
        assert!(res.is_err());
        assert!(!stderr.is_empty());
    }
}
