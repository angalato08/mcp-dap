use std::process::Stdio;

use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::error::AppError;

/// Spawned adapter handle with access to its stdio streams.
pub struct AdapterProcess {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub child: Child,
}

/// Spawn a debug adapter subprocess, capturing stdin/stdout for DAP communication.
pub fn spawn_adapter(path: &str, args: &[String]) -> Result<AdapterProcess, AppError> {
    let mut child = Command::new(path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(AppError::SpawnFailed)?;

    let stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");

    Ok(AdapterProcess {
        stdin,
        stdout,
        child,
    })
}
