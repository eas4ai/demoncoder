//! A blocking owner retains every execution resource until the supervisor exits.
use super::{HookHost, command::failure, launch::Launch};
use crate::{plugins::receipts::RawOutcome, supervisor::HookLaunch, worktree_access::HookCommand};
use anyhow::{Context, Result};
use std::{
    io::{Read, Write},
    os::{fd::AsRawFd, unix::process::ExitStatusExt},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const POLL: Duration = Duration::from_millis(5);

struct OwnedProcess {
    child: Child,
    lease: Option<ChildStdin>,
    status: Option<ExitStatus>,
    launch: Launch,
}
impl OwnedProcess {
    fn revoke(&mut self) {
        self.lease.take();
        self.launch.stop();
    }
    fn observe(&mut self) -> Result<()> {
        if self.status.is_none() {
            self.status = self
                .child
                .try_wait()
                .context("observe confined command owner")?;
        }
        Ok(())
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.revoke();
        // No abort path releases mounted data or the host mutation guard while
        // supervised children can act. A stuck kernel reap retains ownership.
        while self.status.is_none() || !self.launch.stopped().unwrap_or(false) {
            let _ = self.observe();
            self.launch.stop();
            std::thread::sleep(POLL);
        }
    }
}

pub(super) fn run(
    host: &HookHost,
    command: &HookCommand,
    input: &[u8],
    maximum: usize,
    deadline: Instant,
    cancelled: &AtomicBool,
    owner: &dyn Fn() -> Result<()>,
) -> RawOutcome {
    let launch = match Launch::new() {
        Ok(launch) => launch,
        Err(_) => return failure("cannot establish kernel-observed sandbox lifetime"),
    };
    let mut input_file = match tempfile::NamedTempFile::new() {
        Ok(file) => file,
        Err(_) => return failure("cannot stage owned hook input"),
    };
    if input_file.write_all(input).is_err() {
        return failure("cannot write bounded hook event input");
    }
    let encoded = match serde_json::to_string(&HookLaunch {
        arguments: command.launch_arguments(&launch.status_writer, &launch.gate_reader),
        input: format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            input_file.as_file().as_raw_fd()
        ),
    }) {
        Ok(value) if value.len() <= 128 * 1024 => value,
        _ => return failure("hook launch description exceeds bound"),
    };
    if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline || owner().is_err() {
        return failure("hook cancelled or expired before process launch");
    }
    let Some(supervisor) = &host.supervisor else {
        return failure("hook runtime supervisor is missing");
    };
    let child = match Command::new(supervisor)
        .args(["--supervise-hook", &encoded])
        .env_clear()
        .env("TOKIO_WORKER_THREADS", "1")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return failure("cannot execute configured hook supervisor; no fallback"),
    };
    let mut process = OwnedProcess {
        child,
        lease: None,
        status: None,
        launch,
    };
    process.lease = process.child.stdin.take();
    let prepared = (|| -> Result<_> {
        let stdout = process.child.stdout.take().context("missing hook stdout")?;
        let stderr = process.child.stderr.take().context("missing hook stderr")?;
        for fd in [
            &stdout as &dyn std::os::fd::AsFd,
            &stderr as &dyn std::os::fd::AsFd,
        ] {
            let flags = rustix::fs::fcntl_getfl(fd)?;
            rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)?;
        }
        process
            .lease
            .as_mut()
            .context("missing hook lifetime lease")?
            .write_all(b"1")?;
        Ok((stdout, stderr))
    })();
    let (mut stdout, mut stderr) = match prepared {
        Ok(pipes) => pipes,
        Err(_) => {
            return failure("cannot establish confined command lifetime and output ownership");
        }
    };
    let mut output = Output {
        stdout: Vec::new(),
        stderr: Vec::new(),
        maximum,
        problem: None,
    };
    let (mut out_open, mut err_open) = (true, true);
    loop {
        if output.problem.is_none() {
            if cancelled.load(Ordering::Acquire) {
                output.problem = Some("command cancelled; effects may be unknown");
            } else if Instant::now() >= deadline {
                output.problem = Some("command timed out; effects may be unknown");
            } else if owner().is_err() {
                output.problem = Some("command owner held or expired; effects may be unknown");
            }
        }
        if output.problem.is_none() {
            let admit = (|| -> Result<()> {
                if process.launch.prepare(process.child.id())? {
                    anyhow::ensure!(
                        !cancelled.load(Ordering::Acquire) && Instant::now() < deadline,
                        "hook launch cancelled or expired"
                    );
                    owner()?;
                    process.launch.admit()?;
                }
                Ok(())
            })();
            if admit.is_err() {
                output.problem =
                    Some("cannot establish sandbox launch ownership; effects may be unknown");
            }
        }
        // One bounded read from EACH stream per turn; a stdout flood cannot
        // starve stderr, owner checks or cancellation.
        output.read(&mut stdout, true, &mut out_open);
        output.read(&mut stderr, false, &mut err_open);
        if output.problem.is_some() {
            process.revoke();
        }
        if process.observe().is_err() {
            output
                .problem
                .get_or_insert("command owner wait failed; effects may be unknown");
            process.revoke();
        }
        if process.status.is_some() {
            process.revoke();
        }
        if process.status.is_some()
            && process.launch.stopped().unwrap_or(false)
            && !out_open
            && !err_open
        {
            break;
        }
        std::thread::sleep(POLL);
    }
    let status = process.status.expect("observed supervisor exit");
    if let Some(reason) = output.problem {
        RawOutcome::CommandFailure {
            reason: reason.into(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    } else if status.signal().is_some() {
        RawOutcome::CommandFailure {
            reason: "command supervisor was interrupted; effects may be unknown".into(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    } else if !process.launch.admitted() {
        RawOutcome::CommandFailure {
            reason: "sandbox payload was not admitted by its owner".into(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    } else {
        RawOutcome::Command {
            exit_code: status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}
struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    maximum: usize,
    problem: Option<&'static str>,
}
impl Output {
    fn read(&mut self, pipe: &mut impl Read, standard_out: bool, open: &mut bool) {
        if !*open {
            return;
        }
        let mut bytes = [0; 4096];
        match pipe.read(&mut bytes) {
            Ok(0) => *open = false,
            Ok(count) => {
                let available = self
                    .maximum
                    .saturating_sub(self.stdout.len() + self.stderr.len());
                let retained = count.min(available);
                let target = if standard_out {
                    &mut self.stdout
                } else {
                    &mut self.stderr
                };
                target.reserve_exact(retained);
                target.extend_from_slice(&bytes[..retained]);
                if retained < count {
                    self.problem
                        .get_or_insert("command output exceeded bound; effects may be unknown");
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => {
                *open = false;
                self.problem
                    .get_or_insert("command output transport failed; effects may be unknown");
            }
        }
    }
}
