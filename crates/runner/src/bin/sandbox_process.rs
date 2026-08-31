use std::io::{stderr, stdin, stdout};
use verify_sandbox_isolation::DockerBackend;
use verify_sandbox_runner::run_once;

fn main() {
    const MAX_REQUEST_BYTES: usize = 1024 * 1024; // 1 MiB
    let mut stdin = stdin();
    let mut stdout = stdout();
    let mut stderr = stderr();
    let backend = DockerBackend::default();
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
