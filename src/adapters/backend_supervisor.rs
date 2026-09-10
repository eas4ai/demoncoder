//! Surviving stdin lease: owner death cannot turn a pending callback into EOF.
use anyhow::{Context, Result, bail};
use std::{path::PathBuf, process::Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// stderr initially carries the write lease. Move it to a private CLOEXEC
/// descriptor before starting the backend so diagnostics cannot enter SDK input.
pub async fn run(spec: &str) -> Result<()> {
    let lease = rustix::stdio::stderr().try_clone_to_owned()?;
    rustix::stdio::dup2_stderr(std::fs::OpenOptions::new().write(true).open("/dev/null")?)?;
    let _lease = Lease(lease);
    let result = forward(spec).await;
    // This supervisor is the unreaped process-group leader. Killing our own
    // group stops the backend before any lease can close and expose SDK EOF.
    let _ = rustix::process::kill_process_group(
        rustix::process::getpid(),
        rustix::process::Signal::KILL,
    );
    // A failed group kill must not return through main (which logs to stderr),
    // or release stdin. Keep the lease alive; the parent may still clean up.
    let _ = result;
    std::future::pending().await
}

async fn forward(spec: &str) -> Result<()> {
    let (binary, args, owner): (PathBuf, Vec<String>, u32) =
        serde_json::from_str(spec).context("backend supervisor spec")?;
    let owner =
        rustix::process::Pid::from_raw(i32::try_from(owner)?).context("invalid owner PID")?;
    anyhow::ensure!(
        rustix::process::getppid() == Some(owner),
        "backend owner changed before supervision"
    );
    let owner_fd = rustix::process::pidfd_open(owner, rustix::process::PidfdFlags::NONBLOCK)?;
    anyhow::ensure!(
        rustix::process::getppid() == Some(owner),
        "backend owner changed during supervision"
    );
    let owner_exit = tokio::io::unix::AsyncFd::new(owner_fd)?;
    let mut child = tokio::process::Command::new(binary)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("start supervised backend")?;
    let mut input = child.stdout.take().context("supervised stdout")?;
    let mut output = tokio::io::stdout();
    let mut partial = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        tokio::select! {
            biased;
            result = owner_exit.readable() => { let _ = result?; bail!("backend owner exited"); }
            count = input.read(&mut buffer) => {
                let count = count?;
                if count == 0 { bail!("backend stdout closed"); }
                for &byte in &buffer[..count] {
                    partial.push(byte);
                    anyhow::ensure!(partial.len() <= 4 * 1024 * 1024, "backend response too large");
                    if byte == b'\n' {
                        tokio::select! {
                            biased;
                            result = owner_exit.readable() => { let _ = result?; bail!("backend owner exited"); }
                            result = output.write_all(&partial) => result?,
                        }
                        partial.clear();
                    }
                }
            }
        }
    }
}

// Preserve the kill-before-close ordering even if the forwarding future unwinds.
struct Lease(std::os::fd::OwnedFd);
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = &self.0;
        let _ = rustix::process::kill_process_group(
            rustix::process::getpid(),
            rustix::process::Signal::KILL,
        );
    }
}
