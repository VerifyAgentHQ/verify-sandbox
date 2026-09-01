use std::io::{stderr, stdin, stdout};
use std::path::PathBuf;
use verify_sandbox_isolation::{DockerBackend, LocalSnapshotStoreMaterializer};
use verify_sandbox_runner::run_once;

fn main() {
    const MAX_REQUEST_BYTES: usize = 1024 * 1024; // 1 MiB
    let mut stdin = stdin();
    let mut stdout = stdout();
    let mut stderr = stderr();
    let snapshot_root = match std::env::var_os("VERIFY_SANDBOX_SNAPSHOT_ROOT") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => {
            eprintln!("VERIFY_SANDBOX_SNAPSHOT_ROOT is required");
            std::process::exit(2);
        }
    };
    let docker_executable = match std::env::var_os("VERIFY_SANDBOX_DOCKER_EXECUTABLE") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => {
            eprintln!("VERIFY_SANDBOX_DOCKER_EXECUTABLE is required");
            std::process::exit(2);
        }
    };
    if !docker_executable.is_absolute() || !docker_executable.is_file() {
        eprintln!("VERIFY_SANDBOX_DOCKER_EXECUTABLE must be an existing absolute file");
        std::process::exit(2);
    }
    let docker_host = std::env::var("VERIFY_SANDBOX_DOCKER_HOST").ok();
    if docker_host
        .as_deref()
        .is_some_and(|host| host.is_empty() || host.contains('\0'))
    {
        eprintln!("VERIFY_SANDBOX_DOCKER_HOST is invalid");
        std::process::exit(2);
    }
    let system_root = std::env::var("VERIFY_SANDBOX_SYSTEM_ROOT").ok();
    if system_root
        .as_deref()
        .is_some_and(|root| root.is_empty() || root.contains('\0'))
    {
        eprintln!("VERIFY_SANDBOX_SYSTEM_ROOT is invalid");
        std::process::exit(2);
    }
    let temp_root = match std::env::var_os("VERIFY_SANDBOX_TEMP_ROOT") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => {
            eprintln!("VERIFY_SANDBOX_TEMP_ROOT is required");
            std::process::exit(2);
        }
    };
    if !temp_root.is_absolute() || !temp_root.is_dir() {
        eprintln!("VERIFY_SANDBOX_TEMP_ROOT must be an existing absolute directory");
        std::process::exit(2);
    }
    let mut docker_config = verify_sandbox_isolation::DockerConfig::development_with_executable(
        "verify-agent/runner:development".into(),
        docker_executable,
    );
    docker_config.docker_host = docker_host;
    docker_config.system_root = system_root;
    docker_config.temp_root = Some(temp_root);
    let backend = DockerBackend::new(
        docker_config,
        LocalSnapshotStoreMaterializer::new(snapshot_root),
    );
    match run_once(
        &mut stdin,
        &mut stdout,
        &mut stderr,
        backend,
        MAX_REQUEST_BYTES,
    ) {
        Ok(()) => std::process::exit(0),
        Err(code) => std::process::exit(code),
    }
}
