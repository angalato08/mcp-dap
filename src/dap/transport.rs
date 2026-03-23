use std::process::Stdio;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::dap::client::DapClient;
use crate::error::AppError;

/// Spawned adapter handle with access to its stdio streams.
pub struct AdapterProcess {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub child: Child,
}

/// Spawn a debug adapter subprocess, capturing stdin/stdout for DAP communication.
pub fn spawn_adapter(path: &str, args: &[String]) -> Result<AdapterProcess, AppError> {
    let mut std_command = std::process::Command::new(path);
    std_command.args(args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe and has no preconditions.
        // Creates a new session so the adapter and its children cannot
        // read from the parent's controlling terminal (which would send
        // SIGTTIN and suspend the MCP host process).
        unsafe {
            std_command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let mut command = Command::from(std_command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().map_err(AppError::SpawnFailed)?;

    let stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");

    Ok(AdapterProcess {
        stdin,
        stdout,
        child,
    })
}

/// Spawn a TCP-based debug adapter (e.g. delve) and connect to it.
///
/// Launches the adapter with `-l 127.0.0.1:{port}` and connects via TCP.
/// Returns a `DapClient` using the TCP stream for DAP communication.
pub async fn spawn_tcp_adapter(
    path: &str,
    args: &[String],
    port: u16,
) -> Result<DapClient, AppError> {
    let addr = format!("127.0.0.1:{port}");

    let mut full_args = args.to_vec();
    full_args.extend_from_slice(&["-l".into(), addr.clone()]);

    let mut std_command = std::process::Command::new(path);
    std_command.args(&full_args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe and has no preconditions.
        unsafe {
            std_command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let mut command = Command::from(std_command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = command.spawn().map_err(AppError::SpawnFailed)?;

    // Wait for the adapter to start listening.
    let stream = retry_connect(&addr, 20, Duration::from_millis(200)).await?;
    let (read_half, write_half) = stream.into_split();

    Ok(DapClient::from_stream(read_half, write_half, child))
}

/// Retry TCP connection with backoff.
async fn retry_connect(
    addr: &str,
    max_attempts: u32,
    delay: Duration,
) -> Result<TcpStream, AppError> {
    for _ in 0..max_attempts {
        match TcpStream::connect(addr).await {
            Ok(stream) => return Ok(stream),
            Err(_) => tokio::time::sleep(delay).await,
        }
    }
    Err(AppError::DapError(format!(
        "failed to connect to adapter at {addr} after {max_attempts} attempts"
    )))
}
