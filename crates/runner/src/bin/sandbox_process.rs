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
    let backend = DockerBackend::new(
        verify_sandbox_isolation::DockerConfig::development(
            "verify-agent/runner:development".into(),
        ),
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
