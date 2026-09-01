//! Replaceable isolation boundary with the first, Docker-backed execution backend.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use verify_sandbox_protocol::{ArtifactPolicy, NetworkPolicy, SandboxJobRequest, SandboxJobResult};

const MAX_OUTPUT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 50 * 1024 * 1024;
const MAX_WORKSPACE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_MATERIALIZED_FILES: u64 = 100_000;
const MAX_MATERIALIZED_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MATERIALIZED_PATH_BYTES: usize = 4096;
const MAX_TIMEOUT_MS: u64 = 60 * 60 * 1000;

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
            output_limit_bytes: MAX_OUTPUT_BYTES,
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

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("unsafe backend configuration: {0}")]
    UnsafeConfiguration(String),
    #[error("unsupported request: {0}")]
    Unsupported(String),
    #[error("source materialization failed: {0}")]
    Materialization(String),
    #[error("docker operation failed: {0}")]
    Docker(String),
    #[error("I/O failure: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApprovedCommand {
    executable: String,
    #[serde(default)]
    args: Vec<String>,
    working_directory: String,
    #[serde(default)]
    environment: BTreeMap<String, String>,
}

fn parse_commands(request: &SandboxJobRequest) -> Result<Vec<ApprovedCommand>, BackendError> {
    request
        .commands
        .iter()
        .map(|encoded| {
            let command: ApprovedCommand = serde_json::from_str(encoded).map_err(|_| {
                BackendError::InvalidRequest("commands must be JSON argv records".into())
            })?;
            if command.executable.is_empty()
                || command.executable.contains(['/', '\\'])
                || matches!(
                    command.executable.as_str(),
                    "sh" | "bash" | "zsh" | "cmd" | "powershell" | "pwsh"
                )
            {
                return Err(BackendError::InvalidRequest("unsafe executable".into()));
            }
            if !matches!(command.executable.as_str(), "pnpm" | "cargo") {
                return Err(BackendError::Unsupported(format!(
                    "executable {}",
                    command.executable
                )));
            }
            if command.working_directory != "." || !command.environment.is_empty() {
                return Err(BackendError::InvalidRequest(
                    "working directory or environment is not approved".into(),
                ));
            }
            if command.args.iter().any(|arg| arg.contains('\0')) {
                return Err(BackendError::InvalidRequest("argument contains NUL".into()));
            }
            Ok(command)
        })
        .collect()
}

pub trait SourceMaterializer: Send + Sync {
    /// Materialize the configured source snapshot into the supplied empty directory.
    ///
    /// # Errors
    ///
    /// Returns an error when source materialization cannot be completed safely.
    fn materialize(&self, snapshot: &str, destination: &Path) -> Result<(), BackendError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct EmptySourceMaterializer;

impl SourceMaterializer for EmptySourceMaterializer {
    fn materialize(&self, _snapshot: &str, _destination: &Path) -> Result<(), BackendError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MaterializationLimits {
    pub max_total_bytes: u64,
    pub max_files: u64,
    pub max_file_bytes: u64,
    pub max_path_bytes: usize,
}

impl Default for MaterializationLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: MAX_WORKSPACE_BYTES,
            max_files: MAX_MATERIALIZED_FILES,
            max_file_bytes: MAX_MATERIALIZED_FILE_BYTES,
            max_path_bytes: MAX_MATERIALIZED_PATH_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalSnapshotStoreMaterializer {
    root: PathBuf,
    limits: MaterializationLimits,
}

impl LocalSnapshotStoreMaterializer {
    /// Creates a materializer whose snapshot IDs resolve beneath `root` only.
    /// The caller must supply this root from trusted operator configuration.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            limits: MaterializationLimits::default(),
        }
    }

    #[must_use]
    pub fn with_limits(root: PathBuf, limits: MaterializationLimits) -> Self {
        Self { root, limits }
    }

    fn source_for(&self, snapshot: &str) -> Result<PathBuf, BackendError> {
        if snapshot.is_empty()
            || snapshot.len() > 256
            || !snapshot.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
            || snapshot == "."
            || snapshot == ".."
        {
            return Err(BackendError::InvalidRequest("unsafe snapshot id".into()));
        }
        if is_indirection(&fs::symlink_metadata(&self.root)?) {
            return Err(BackendError::Materialization(
                "snapshot store indirection rejected".into(),
            ));
        }
        let root = fs::canonicalize(&self.root)
            .map_err(|_| BackendError::Materialization("snapshot store unavailable".into()))?;
        let source = root.join(snapshot);
        if is_indirection(&fs::symlink_metadata(&source)?) {
            return Err(BackendError::Materialization(
                "source snapshot indirection rejected".into(),
            ));
        }
        let canonical = fs::canonicalize(&source)
            .map_err(|_| BackendError::Materialization("source snapshot not found".into()))?;
        if !canonical.starts_with(&root) || is_indirection(&fs::symlink_metadata(&canonical)?) {
            return Err(BackendError::Materialization(
                "unsafe source snapshot".into(),
            ));
        }
        Ok(canonical)
    }
}

impl SourceMaterializer for LocalSnapshotStoreMaterializer {
    fn materialize(&self, snapshot: &str, destination: &Path) -> Result<(), BackendError> {
        let source = self.source_for(snapshot)?;
        if is_indirection(&fs::symlink_metadata(destination)?) {
            return Err(BackendError::Materialization(
                "destination indirection rejected".into(),
            ));
        }
        copy_tree_with_limits(&source, destination, self.limits)
            .map_err(|error| BackendError::Materialization(error.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct DirectorySourceMaterializer {
    root: PathBuf,
}

impl DirectorySourceMaterializer {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl SourceMaterializer for DirectorySourceMaterializer {
    fn materialize(&self, _snapshot: &str, destination: &Path) -> Result<(), BackendError> {
        if is_indirection(&fs::symlink_metadata(&self.root)?) {
            return Err(BackendError::Materialization(
                "source root symlink rejected".into(),
            ));
        }
        copy_tree_with_limits(&self.root, destination, MaterializationLimits::default())
            .map_err(|error| BackendError::Materialization(error.to_string()))
    }
}

fn copy_tree_with_limits(
    source: &Path,
    destination: &Path,
    limits: MaterializationLimits,
) -> io::Result<()> {
    fn copy_tree_bounded(
        source: &Path,
        destination: &Path,
        used: &mut u64,
        files: &mut u64,
        limits: MaterializationLimits,
    ) -> io::Result<()> {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if is_indirection(&metadata) {
                return Err(io::Error::other("source symlink or reparse point rejected"));
            }
            let target = destination.join(entry.file_name());
            let relative = target
                .strip_prefix(destination)
                .map_err(|_| io::Error::other("destination path escaped root"))?;
            if relative.as_os_str().to_string_lossy().len() > limits.max_path_bytes {
                return Err(io::Error::other("source path length exceeded"));
            }
            if metadata.file_type().is_dir() {
                fs::create_dir_all(&target)?;
                copy_tree_bounded(&entry.path(), &target, used, files, limits)?;
            } else if metadata.file_type().is_file() {
                if metadata.len() > limits.max_file_bytes {
                    return Err(io::Error::other("source file size exceeded"));
                }
                *files = files
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("source file count overflow"))?;
                if *files > limits.max_files {
                    return Err(io::Error::other("source file count exceeded"));
                }
                *used = used
                    .checked_add(metadata.len())
                    .ok_or_else(|| io::Error::other("source size overflow"))?;
                if *used > limits.max_total_bytes {
                    return Err(io::Error::other("source workspace size exceeded"));
                }
                fs::copy(entry.path(), target)?;
            } else {
                return Err(io::Error::other("unsupported source entry"));
            }
        }
        Ok(())
    }

    let mut used = 0;
    let mut files = 0;
    copy_tree_bounded(source, destination, &mut used, &mut files, limits)
}

#[cfg(windows)]
fn is_indirection(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_indirection(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[derive(Debug, Clone)]
pub struct DockerConfig {
    pub docker_executable: PathBuf,
    pub image: String,
    pub pids_limit: u64,
    pub cpu_limit: f64,
    pub output_limit_bytes: u64,
    pub artifact_limit_bytes: u64,
    pub max_timeout_ms: u64,
    pub max_memory_limit_bytes: u64,
}

impl DockerConfig {
    #[must_use]
    pub fn development(image: String) -> Self {
        Self {
            docker_executable: PathBuf::from("docker"),
            image,
            pids_limit: 256,
            cpu_limit: 1.0,
            output_limit_bytes: MAX_OUTPUT_BYTES,
            artifact_limit_bytes: MAX_ARTIFACT_BYTES,
            max_timeout_ms: MAX_TIMEOUT_MS,
            max_memory_limit_bytes: 4 * 1024 * 1024 * 1024,
        }
    }

    fn validate(&self) -> Result<(), BackendError> {
        if self.image.trim().is_empty()
            || !self
                .image
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphanumeric())
            || self.image.chars().any(|character| {
                !(character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '_' | '/' | ':' | '-' | '@'))
            })
        {
            return Err(BackendError::UnsafeConfiguration(
                "approved image is invalid".into(),
            ));
        }
        if self.pids_limit == 0
            || !self.cpu_limit.is_finite()
            || self.cpu_limit <= 0.0
            || self.output_limit_bytes == 0
            || self.output_limit_bytes > MAX_OUTPUT_BYTES
            || self.max_timeout_ms == 0
            || self.max_timeout_ms > MAX_TIMEOUT_MS
            || self.max_memory_limit_bytes == 0
            || self.max_memory_limit_bytes > 1_099_511_627_776
        {
            return Err(BackendError::UnsafeConfiguration(
                "resource bounds are invalid".into(),
            ));
        }
        Ok(())
    }
}

pub struct DockerBackend<M = EmptySourceMaterializer> {
    pub config: DockerConfig,
    pub materializer: M,
}

impl Default for DockerBackend<EmptySourceMaterializer> {
    fn default() -> Self {
        Self {
            config: DockerConfig::development("verify-agent/runner:development".into()),
            materializer: EmptySourceMaterializer,
        }
    }
}

impl<M: SourceMaterializer> DockerBackend<M> {
    #[must_use]
    pub fn new(config: DockerConfig, materializer: M) -> Self {
        Self {
            config,
            materializer,
        }
    }

    fn container_name(job_id: &str) -> Result<String, BackendError> {
        if job_id.is_empty()
            || job_id.len() > 128
            || !job_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(BackendError::InvalidRequest("unsafe job id".into()));
        }
        Ok(format!("verify-agent-{job_id}"))
    }

    fn docker(&self, args: &[String]) -> Result<std::process::Output, BackendError> {
        Command::new(&self.config.docker_executable)
            .args(args)
            .env_clear()
            .output()
            .map_err(BackendError::Io)
    }

    fn cleanup(&self, name: &str) -> Result<(), BackendError> {
        let removed = self.docker(&["rm".into(), "--force".into(), name.into()])?;
        if removed.status.success()
            || String::from_utf8_lossy(&removed.stderr).contains("No such container")
        {
            Ok(())
        } else {
            Err(BackendError::Docker(
                String::from_utf8_lossy(&removed.stderr).into_owned(),
            ))
        }
    }

    /// Execute a validated request in an ephemeral, non-privileged Docker container.
    ///
    /// # Errors
    ///
    /// Returns an error before Docker starts when the request or backend configuration is unsafe,
    /// or when provisioning/materialization fails.
    pub fn execute(
        &self,
        request: &SandboxJobRequest,
        cancellation: Option<&CancellationToken>,
    ) -> Result<SandboxJobResult, BackendError> {
        request
            .validate()
            .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
        self.config.validate()?;
        if request.network_policy != NetworkPolicy::None {
            return Err(BackendError::UnsafeConfiguration(
                "network policy is not safely supported".into(),
            ));
        }
        if request.artifact_policy == ArtifactPolicy::Declared {
            return Err(BackendError::Unsupported(
                "declared artifact paths are absent from the approved contract".into(),
            ));
        }
        if request.resource_limits.timeout_ms > self.config.max_timeout_ms
            || request.resource_limits.memory_limit_bytes > self.config.max_memory_limit_bytes
        {
            return Err(BackendError::UnsafeConfiguration(
                "request exceeds backend resource bounds".into(),
            ));
        }
        let commands = parse_commands(request)?;
        let name = Self::container_name(&request.job_id)?;
        let workspace = unique_temp_dir(&request.job_id)?;
        let result = match self.execute_inner(request, &commands, &name, &workspace, cancellation) {
            Ok(result) => result,
            Err(error) => error_result(request, &error),
        };
        let workspace_cleanup = fs::remove_dir_all(&workspace);
        let container_cleanup = self.cleanup(&name);
        if let Err(error) = workspace_cleanup {
            return Err(BackendError::Io(error));
        }
        container_cleanup?;
        Ok(result)
    }

    #[allow(clippy::too_many_lines)]
    fn execute_inner(
        &self,
        request: &SandboxJobRequest,
        commands: &[ApprovedCommand],
        name: &str,
        workspace: &Path,
        cancellation: Option<&CancellationToken>,
    ) -> Result<SandboxJobResult, BackendError> {
        self.materializer
            .materialize(&request.snapshot, workspace)?;
        let create = vec![
            "create".into(),
            "--name".into(),
            name.into(),
            "--network".into(),
            "none".into(),
            "--read-only".into(),
            "--memory".into(),
            request.resource_limits.memory_limit_bytes.to_string(),
            "--cpus".into(),
            self.config.cpu_limit.to_string(),
            "--pids-limit".into(),
            self.config.pids_limit.to_string(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges:true".into(),
            "--user".into(),
            "65532:65532".into(),
            "--tmpfs".into(),
            "/workspace:rw,nosuid,nodev,size=1g,uid=65532,gid=65532,mode=700".into(),
            "--tmpfs".into(),
            "/cargo-home:rw,nosuid,nodev,size=512m".into(),
            "--tmpfs".into(),
            "/pnpm-home:rw,nosuid,nodev,size=512m".into(),
            "--tmpfs".into(),
            "/tmp:rw,nosuid,nodev,size=256m".into(),
            "--env".into(),
            "HOME=/tmp/home".into(),
            "--env".into(),
            "CARGO_HOME=/cargo-home".into(),
            "--env".into(),
            "PNPM_HOME=/pnpm-home".into(),
            "--env".into(),
            "npm_config_cache=/pnpm-home/npm-cache".into(),
            "--env".into(),
            "XDG_CACHE_HOME=/pnpm-home/xdg-cache".into(),
            self.config.image.clone(),
            "sleep".into(),
            "2147483647".into(),
        ];
        let created = self.docker(&create)?;
        if !created.status.success() {
            return Err(BackendError::Docker(
                String::from_utf8_lossy(&created.stderr).into_owned(),
            ));
        }
        let started = self.docker(&["start".into(), name.into()])?;
        if !started.status.success() {
            return Err(BackendError::Docker(
                String::from_utf8_lossy(&started.stderr).into_owned(),
            ));
        }
        copy_workspace(&self.config.docker_executable, name, workspace)?;
        let total_started = Instant::now();
        let mut truncated = false;
        for command in commands {
            let mut args = vec![
                "exec".into(),
                "--workdir".into(),
                "/workspace".into(),
                name.into(),
                command.executable.clone(),
            ];
            args.extend(command.args.clone());
            let mut child = Command::new(&self.config.docker_executable)
                .args(args)
                .env_clear()
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| BackendError::Docker("stdout pipe unavailable".into()))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| BackendError::Docker("stderr pipe unavailable".into()))?;
            let output_limit =
                usize::try_from(self.config.output_limit_bytes / 2).map_err(|_| {
                    BackendError::UnsafeConfiguration("output limit is not representable".into())
                })?;
            let out_thread = thread::spawn(move || read_bounded(stdout, output_limit));
            let err_thread = thread::spawn(move || read_bounded(stderr, output_limit));
            let status = wait_with_deadline(
                &mut child,
                Duration::from_millis(request.resource_limits.timeout_ms),
                cancellation,
            )?;
            let out = out_thread
                .join()
                .map_err(|_| BackendError::Docker("stdout collector panicked".into()))??;
            let err = err_thread
                .join()
                .map_err(|_| BackendError::Docker("stderr collector panicked".into()))??;
            let duration_ms = elapsed_ms(total_started);
            truncated |= out.truncated || err.truncated;
            if status == WaitOutcome::TimedOut {
                return Ok(result(request, "timed_out", None, duration_ms, truncated));
            }
            if status == WaitOutcome::Cancelled {
                return Ok(result(request, "cancelled", None, duration_ms, truncated));
            }
            let exit = status_code(status);
            if exit != Some(0) {
                return Ok(result(request, "completed", exit, duration_ms, truncated));
            }
        }
        Ok(result(
            request,
            "completed",
            Some(0),
            elapsed_ms(total_started),
            truncated,
        ))
    }
}

fn copy_workspace(
    docker_executable: &Path,
    name: &str,
    workspace: &Path,
) -> Result<(), BackendError> {
    // docker cp writes the container rootfs directly and fails with
    // --read-only, even when /workspace is a writable tmpfs. Extract through
    // docker exec so writes land in the explicitly mounted tmpfs.
    let mut child = Command::new(docker_executable)
        .args([
            "exec",
            "--interactive",
            name,
            "tar",
            "-x",
            "-f",
            "-",
            "-C",
            "/workspace",
        ])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| BackendError::Docker("workspace transfer pipe unavailable".into()))?;
    write_tar(workspace, &mut input)?;
    drop(input);
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(BackendError::Docker(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(())
}

fn write_tar(root: &Path, output: &mut impl io::Write) -> Result<(), BackendError> {
    fn visit(root: &Path, path: &Path, output: &mut impl io::Write) -> Result<(), BackendError> {
        let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let entry_path = entry.path();
            let relative = entry_path
                .strip_prefix(root)
                .map_err(|_| BackendError::Materialization("workspace path escaped root".into()))?;
            let name = relative.to_string_lossy().replace('\\', "/");
            let metadata = fs::symlink_metadata(&entry_path)?;
            validate_archive_path(&name)?;
            if metadata.file_type().is_dir() {
                tar_path_header(output, &name, 0, b'5')?;
                visit(root, &entry_path, output)?;
            } else if metadata.file_type().is_file() {
                tar_path_header(output, &name, metadata.len(), b'0')?;
                let mut file = fs::File::open(entry_path)?;
                io::copy(&mut file, output)?;
                pad_tar(output, metadata.len())?;
            } else {
                return Err(BackendError::Materialization(
                    "unsupported workspace entry".into(),
                ));
            }
        }
        Ok(())
    }

    visit(root, root, output)?;
    output.write_all(&[0; 1024])?;
    Ok(())
}

fn validate_archive_path(name: &str) -> Result<(), BackendError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains(':')
        || name
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(BackendError::Materialization(
            "workspace path is not a normalized relative path".into(),
        ));
    }
    Ok(())
}

fn tar_path_header(
    output: &mut impl io::Write,
    name: &str,
    size: u64,
    kind: u8,
) -> Result<(), BackendError> {
    validate_archive_path(name)?;
    if name.len() > 100 {
        let record = pax_path_record(name);
        tar_header(output, "PaxHeaders.0", record.len() as u64, b'x')?;
        output.write_all(&record)?;
        pad_tar(output, record.len() as u64)?;
        tar_header(output, "pax-entry", size, kind)?;
    } else {
        tar_header(output, name, size, kind)?;
    }
    Ok(())
}

fn pax_path_record(name: &str) -> Vec<u8> {
    let payload = format!("path={name}\n");
    let mut length = payload.len() + 2;
    loop {
        let candidate = format!("{length} {payload}");
        if candidate.len() == length {
            return candidate.into_bytes();
        }
        length = candidate.len();
    }
}

fn tar_header(
    output: &mut impl io::Write,
    name: &str,
    size: u64,
    kind: u8,
) -> Result<(), BackendError> {
    let name_bytes = name.as_bytes();
    if name_bytes.len() > 100 {
        return Err(BackendError::Materialization(
            "tar header name is too long".into(),
        ));
    }
    let mut header = [0_u8; 512];
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    write_octal(&mut header[100..108], 0o755);
    write_octal(&mut header[108..116], 65532);
    write_octal(&mut header[116..124], 65532);
    write_octal(&mut header[124..136], size);
    write_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    write_octal(&mut header[148..156], u64::from(checksum));
    output.write_all(&header)?;
    Ok(())
}

fn write_octal(field: &mut [u8], value: u64) {
    field.fill(b'0');
    let digits = format!("{value:o}");
    let start = field.len().saturating_sub(digits.len() + 1);
    field[start..start + digits.len()].copy_from_slice(digits.as_bytes());
    field[field.len() - 1] = 0;
}

fn pad_tar(output: &mut impl io::Write, size: u64) -> Result<(), BackendError> {
    let padding = (512 - (size % 512)) % 512;
    if padding > 0 {
        let padding = usize::try_from(padding)
            .map_err(|_| BackendError::Materialization("tar padding overflow".into()))?;
        output.write_all(&vec![0; padding])?;
    }
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn unique_temp_dir(job_id: &str) -> Result<PathBuf, BackendError> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| BackendError::Materialization("clock failure".into()))?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("verify-agent-{job_id}-{nonce}"));
    fs::create_dir(&path)?;
    Ok(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    Exited(Option<i32>),
    TimedOut,
    Cancelled,
}

fn status_code(status: WaitOutcome) -> Option<i32> {
    match status {
        WaitOutcome::Exited(code) => code,
        _ => None,
    }
}

fn wait_with_deadline(
    child: &mut Child,
    timeout: Duration,
    cancellation: Option<&CancellationToken>,
) -> Result<WaitOutcome, BackendError> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancellation.is_some_and(CancellationToken::is_cancelled) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(WaitOutcome::Cancelled);
        }
        if let Some(status) = child.try_wait()? {
            return Ok(WaitOutcome::Exited(status.code()));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(WaitOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

struct BoundedOutput {
    truncated: bool,
}
fn read_bounded<R: Read>(mut reader: R, limit: usize) -> io::Result<BoundedOutput> {
    let mut retained = 0_usize;
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(retained);
        retained += read.min(remaining);
        if read > remaining {
            truncated = true;
        }
    }
    Ok(BoundedOutput { truncated })
}

fn result(
    request: &SandboxJobRequest,
    status: &str,
    exit_code: Option<i32>,
    duration_ms: u64,
    truncated: bool,
) -> SandboxJobResult {
    SandboxJobResult {
        schema_version: "1.0.0".into(),
        job_id: request.job_id.clone(),
        status: match status {
            "completed" => verify_sandbox_protocol::SandboxStatus::Completed,
            "timed_out" => verify_sandbox_protocol::SandboxStatus::TimedOut,
            "cancelled" => verify_sandbox_protocol::SandboxStatus::Cancelled,
            "failed" => verify_sandbox_protocol::SandboxStatus::Failed,
            _ => verify_sandbox_protocol::SandboxStatus::Error,
        },
        exit_code,
        duration_ms,
        logs_ref: format!(
            "docker://{}/logs{}",
            request.job_id,
            if truncated { "?truncated=true" } else { "" }
        ),
        artifact_refs: Vec::new(),
        resource_usage: verify_sandbox_protocol::ResourceUsage {
            memory_bytes: 0,
            cpu_time_ms: 0,
        },
        errors: if truncated {
            vec!["output truncated".into()]
        } else {
            Vec::new()
        },
    }
}

fn error_result(request: &SandboxJobRequest, error: &BackendError) -> SandboxJobResult {
    let mut result = result(request, "error", None, 0, false);
    result.errors.push(error.to_string());
    result
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub trait ExecutionBackend {
    /// # Errors
    ///
    /// Returns an error when request or isolation validation fails.
    fn provision(&self, request: &SandboxJobRequest) -> Result<IsolationConfig, &'static str>;
    /// # Errors
    ///
    /// Returns an error when validation, provisioning, or execution fails.
    fn execute(
        &self,
        request: &SandboxJobRequest,
        cancellation: Option<&CancellationToken>,
    ) -> Result<SandboxJobResult, BackendError>;
}

impl<M: SourceMaterializer> ExecutionBackend for DockerBackend<M> {
    fn provision(&self, request: &SandboxJobRequest) -> Result<IsolationConfig, &'static str> {
        request.validate().map_err(|_| "invalid request")?;
        let config = IsolationConfig::safe_default(PathBuf::from("/ephemeral/workspace"));
        config.validate()?;
        Ok(config)
    }
    fn execute(
        &self,
        request: &SandboxJobRequest,
        cancellation: Option<&CancellationToken>,
    ) -> Result<SandboxJobResult, BackendError> {
        self.execute(request, cancellation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verify_sandbox_protocol::{ResourceLimits, SourceReference};

    fn request(command: &str) -> SandboxJobRequest {
        SandboxJobRequest {
            schema_version: "1.0.0".into(),
            job_id: "job-1".into(),
            source: SourceReference {
                provider: "source".into(),
                reference: "repo".into(),
            },
            snapshot: "snapshot-1".into(),
            commands: vec![command.into()],
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
    fn command_parser_rejects_shell_strings_and_accepts_argv_records() {
        assert!(parse_commands(&request("cargo test && whoami")).is_err());
        let command =
            r#"{"executable":"cargo","args":["test"],"workingDirectory":".","environment":{}}"#;
        assert!(parse_commands(&request(command)).is_ok());
    }

    #[test]
    fn network_and_declared_artifacts_fail_closed() {
        let command =
            r#"{"executable":"cargo","args":["test"],"workingDirectory":".","environment":{}}"#;
        let mut network = request(command);
        network.network_policy = NetworkPolicy::Restricted;
        assert!(DockerBackend::default().execute(&network, None).is_err());
        let mut artifacts = request(command);
        artifacts.artifact_policy = ArtifactPolicy::Declared;
        assert!(DockerBackend::default().execute(&artifacts, None).is_err());
    }

    #[test]
    fn backend_rejects_requests_above_configured_limits() {
        let command =
            r#"{"executable":"cargo","args":["test"],"workingDirectory":".","environment":{}}"#;
        let backend = DockerBackend::default();
        let mut request = request(command);
        request.resource_limits.timeout_ms = backend.config.max_timeout_ms + 1;
        assert!(backend.execute(&request, None).is_err());
    }

    #[test]
    fn image_configuration_cannot_become_a_docker_flag() {
        let mut backend = DockerBackend::default();
        backend.config.image = "--privileged".into();
        let command =
            r#"{"executable":"cargo","args":["test"],"workingDirectory":".","environment":{}}"#;
        assert!(backend.execute(&request(command), None).is_err());
    }

    #[test]
    fn local_snapshot_store_materializes_only_valid_snapshot_ids() {
        let base =
            std::env::temp_dir().join(format!("verify-sandbox-store-{}", std::process::id()));
        let store = base.join("store");
        let destination = base.join("destination");
        fs::create_dir_all(store.join("snap-1/src")).expect("snapshot directory");
        fs::create_dir_all(&destination).expect("destination directory");
        fs::write(store.join("snap-1/package.json"), b"{}\n").expect("source file");
        fs::write(store.join("snap-1/src/index.ts"), b"export {};\n").expect("source file");
        let materializer = LocalSnapshotStoreMaterializer::new(store.clone());
        materializer
            .materialize("snap-1", &destination)
            .expect("materialization");
        assert_eq!(fs::read(destination.join("package.json")).unwrap(), b"{}\n");
        assert_eq!(
            fs::read(destination.join("src/index.ts")).unwrap(),
            b"export {};\n"
        );
        assert!(materializer.materialize("unknown", &destination).is_err());
        for id in [
            "../snap-1",
            "/tmp/snap-1",
            "C:/snap-1",
            "\\\\server\\snap-1",
        ] {
            assert!(
                materializer.materialize(id, &destination).is_err(),
                "accepted {id}"
            );
        }
        assert_eq!(
            fs::read(store.join("snap-1/package.json")).unwrap(),
            b"{}\n"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn materialization_limits_are_enforced() {
        let base =
            std::env::temp_dir().join(format!("verify-sandbox-limits-{}", std::process::id()));
        let source = base.join("source");
        let destination = base.join("destination");
        fs::create_dir_all(&source).expect("source directory");
        fs::create_dir_all(&destination).expect("destination directory");
        fs::write(source.join("one"), b"12345").expect("source file");
        let limits = MaterializationLimits {
            max_total_bytes: 4,
            max_files: 1,
            max_file_bytes: 4,
            max_path_bytes: 32,
        };
        assert!(copy_tree_with_limits(&source, &destination, limits).is_err());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn archive_paths_are_normalized_relative_paths() {
        for path in [
            "../x",
            "/x",
            "C:/x",
            "//server/x",
            "nested/../x",
            "nested\\x",
        ] {
            assert!(
                validate_archive_path(path).is_err(),
                "accepted unsafe path: {path}"
            );
        }
        assert!(validate_archive_path("nested/path.txt").is_ok());
    }

    #[test]
    fn tar_supports_long_paths_and_is_deterministic() {
        let root = std::env::temp_dir().join(format!("verify-sandbox-tar-{}", std::process::id()));
        let below_limit = "a".repeat(99);
        let above_limit = "b".repeat(101);
        let nested = root.join("deep").join("repository").join("relative");
        fs::create_dir_all(&nested).expect("nested source directory");
        fs::write(root.join(&below_limit), b"below").expect("short source file");
        fs::write(nested.join(&above_limit), b"above").expect("long source file");

        let mut first = Vec::new();
        let mut second = Vec::new();
        write_tar(&root, &mut first).expect("first tar archive");
        write_tar(&root, &mut second).expect("second tar archive");
        assert_eq!(first, second);
        assert!(String::from_utf8_lossy(&first)
            .contains(&format!("path=deep/repository/relative/{above_limit}")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "requires the approved local runner image and Docker"]
    fn docker_backend_transfers_a_long_path() {
        let root =
            std::env::temp_dir().join(format!("verify-sandbox-docker-{}", std::process::id()));
        let nested = root.join("nested").join("repository").join("relative");
        let long_name = "long-file-name-".to_owned() + &"x".repeat(90) + ".txt";
        fs::create_dir_all(&nested).expect("source directory");
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = \"long-path-smoke\"\nversion = \"0.1.0\"\n\n[[bin]]\nname = \"long-path-smoke\"\npath = \"nested/repository/relative/{long_name}\"\n"
            ),
        )
        .expect("manifest");
        fs::write(nested.join(&long_name), "fn main() {}\n").expect("long path file");

        let backend = DockerBackend::new(
            DockerConfig::development("verify-agent/runner:development".into()),
            DirectorySourceMaterializer::new(root.clone()),
        );
        let command = r#"{"executable":"cargo","args":["check","--offline","--manifest-path","Cargo.toml"],"workingDirectory":".","environment":{}}"#;
        let mut smoke_request = request(command);
        smoke_request.resource_limits.timeout_ms = 30_000;
        smoke_request.resource_limits.memory_limit_bytes = 1024 * 1024 * 1024;
        let result = backend
            .execute(&smoke_request, None)
            .expect("Docker execution");
        assert_eq!(
            result.status,
            verify_sandbox_protocol::SandboxStatus::Completed,
            "backend errors: {:?}",
            result.errors
        );
        assert_eq!(
            result.exit_code,
            Some(0),
            "backend errors: {:?}",
            result.errors
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn nested_symlinks_are_rejected_before_copying() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("verify-sandbox-test-{}", std::process::id()));
        let source = base.join("source");
        let destination = base.join("destination");
        fs::create_dir_all(source.join("nested")).expect("source directory");
        fs::create_dir_all(&destination).expect("destination directory");
        fs::write(source.join("nested/file.txt"), b"safe").expect("source file");
        symlink(source.join("nested"), source.join("escape")).expect("symlink");

        let result = DirectorySourceMaterializer::new(source).materialize("snapshot", &destination);
        assert!(result.is_err());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn cancellation_token_is_deterministic() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }
}
