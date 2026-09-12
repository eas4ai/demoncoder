//! Host access keeps its permissions; this process owns only Bash's lifetime.
use std::{
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use rustix::{
    fs::{FileType, Mode, OFlags, fstat, open},
    io::{Errno, read},
    process::{
        Pid, Signal, WaitId, WaitIdOptions, getpid, kill_process, set_child_subreaper, waitid,
    },
};

const POLL: Duration = Duration::from_millis(10);

/// Only the runtime holds the lifetime pipe's writer. Nonblocking reads avoid
/// Tokio stdin's uncancellable blocking thread when the handshake times out.
pub async fn run(script: &str) -> Result<i32> {
    let mut command = Command::new("/bin/bash");
    command
        .args(["--noprofile", "--norc", "-c", script])
        .stdin(Stdio::null());
    supervise(command).await
}

/// Bounded host-created launch description. Stdin belongs to the event JSON;
/// the supervisor alone reads the separate lifetime lease on its own fd 0.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HookLaunch {
    pub(crate) arguments: Vec<String>,
    pub(crate) input: String,
    #[serde(default)]
    pub(crate) duplex: bool,
}
pub async fn run_hook(encoded: &str) -> Result<i32> {
    ensure!(
        encoded.len() <= 128 * 1024,
        "hook launch description exceeds bound"
    );
    let spec: HookLaunch =
        serde_json::from_str(encoded).context("invalid hook launch description")?;
    ensure!(
        spec.arguments.len() <= 1024
            && spec
                .arguments
                .iter()
                .all(|v| v.len() <= 65536 && !v.contains('\0')),
        "invalid hook launch arguments"
    );
    let parts = spec.input.split('/').collect::<Vec<_>>();
    ensure!(
        parts.len() == 5
            && parts[0].is_empty()
            && parts[1] == "proc"
            && parts[3] == "fd"
            && [parts[2], parts[4]]
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "hook input requires an owned descriptor"
    );
    let input = std::fs::File::open(&spec.input).context("open owned hook event input")?;
    ensure!(
        if spec.duplex {
            FileType::from_raw_mode(fstat(&input)?.st_mode) == FileType::Fifo
        } else {
            input.metadata()?.is_file() && input.metadata()?.len() <= 65536
        },
        "hook input has a different admitted descriptor type or exceeds bound"
    );
    let mut command = Command::new("/bin/bash");
    command
        .args(spec.arguments)
        .stdin(Stdio::from(input))
        .env_clear();
    supervise(command).await
}

async fn supervise(mut command: Command) -> Result<i32> {
    let lifetime = open(
        "/proc/self/fd/0",
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .context("open host supervisor lifetime pipe")?;
    ensure!(
        FileType::from_raw_mode(fstat(&lifetime)?.st_mode) == FileType::Fifo,
        "host supervisor requires a lifetime pipe"
    );
    let handshake_deadline = Instant::now() + Duration::from_secs(1);
    let mut byte = [0u8];
    loop {
        match read(&lifetime, &mut byte) {
            Ok(1) if byte == *b"1" => break,
            Ok(_) => bail!("invalid host supervisor handshake"),
            Err(Errno::AGAIN | Errno::INTR) => {}
            Err(error) => return Err(error).context("read host supervisor handshake"),
        }
        ensure!(
            Instant::now() < handshake_deadline,
            "host supervisor handshake timed out"
        );
        tokio::time::sleep(POLL).await;
    }
    // Orphans, including setsid/double-fork children, are adopted here rather
    // than escaping to init. This changes parentage, not host permissions.
    set_child_subreaper(Some(getpid())).context("own orphaned host descendants")?;
    direct_children().context("inspect host supervisor child ownership")?;
    let mut child = command
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .process_group(0)
        .spawn()
        .context("start supervised host Bash")?;
    let mut children = Children(true);
    loop {
        match read(&lifetime, &mut byte) {
            // EOF and unexpected additional bytes both revoke the lifetime.
            Ok(_) => {
                children.stop()?;
                return Ok(143);
            }
            Err(Errno::AGAIN | Errno::INTR) => {}
            Err(error) => return Err(error).context("read host supervisor lifetime"),
        }
        if let Some(status) = child.try_wait().context("observe host Bash")? {
            children.stop()?;
            return Ok(status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)));
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Read only this process's direct children. No other code in the supervisor
/// reaps them: std::process::Child has no Tokio background reaper. Until we reap
/// a listed child, its PID cannot be recycled into an unrelated process.
fn direct_children() -> Result<Vec<Pid>> {
    let mut children = Vec::new();
    for task in std::fs::read_dir("/proc/self/task")? {
        let path = task?.path().join("children");
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("read owned host children"),
        };
        for pid in contents.split_whitespace() {
            children.push(Pid::from_raw(pid.parse()?).context("invalid owned host child PID")?);
        }
    }
    Ok(children)
}

struct Children(bool);
impl Children {
    fn stop(&mut self) -> Result<()> {
        if !self.0 {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            for pid in direct_children()? {
                match kill_process(pid, Signal::KILL) {
                    Ok(()) | Err(Errno::SRCH) => {}
                    Err(error) => return Err(error).context("stop owned host child"),
                }
            }
            loop {
                match waitid(WaitId::All, WaitIdOptions::EXITED | WaitIdOptions::NOHANG) {
                    Ok(Some(_)) | Err(Errno::INTR) => continue,
                    Err(Errno::CHILD) => {
                        self.0 = false;
                        // Report a missed deadline only after all descendants
                        // are gone. Exiting early would release surviving work.
                        ensure!(
                            Instant::now() < deadline,
                            "host descendants took more than two seconds to stop"
                        );
                        return Ok(());
                    }
                    Ok(None) => break,
                    Err(error) => return Err(error).context("reap owned host children"),
                }
            }
            // Killing a parent adopts its remaining descendants. Repeat until
            // the kernel reports no children. A fixed sleep per generation
            // makes a deep, finite tree exceed the deadline unnecessarily.
            if Instant::now() < deadline {
                std::thread::yield_now();
            } else {
                // Retain ownership even if a child is slow to acknowledge
                // SIGKILL; avoid busy-waiting after the reporting deadline.
                std::thread::sleep(POLL);
            }
        }
    }
}
impl Drop for Children {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
