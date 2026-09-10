//! A kernel-observed namespace owner is pinned before package code can start.
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::OwnedFd,
};

pub(super) struct Launch {
    status: File,
    pub status_writer: File,
    pub gate_reader: File,
    gate: Option<File>,
    bytes: Vec<u8>,
    namespace: Option<OwnedFd>,
    admitted: bool,
}
impl Launch {
    pub fn new() -> Result<Self> {
        let (status, status_writer) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?;
        let (gate_reader, gate) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?;
        let flags = rustix::fs::fcntl_getfl(&status)?;
        rustix::fs::fcntl_setfl(&status, flags | rustix::fs::OFlags::NONBLOCK)?;
        Ok(Self {
            status: status.into(),
            status_writer: status_writer.into(),
            gate_reader: gate_reader.into(),
            gate: Some(gate.into()),
            bytes: Vec::new(),
            namespace: None,
            admitted: false,
        })
    }
    pub fn prepare(&mut self, supervisor: u32) -> Result<bool> {
        if self.admitted {
            return Ok(false);
        }
        let mut buffer = [0; 1024];
        match self.status.read(&mut buffer) {
            Ok(count) => {
                ensure!(
                    self.bytes.len() + count <= 4096,
                    "sandbox launch status exceeds bound"
                );
                self.bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error).context("read sandbox launch status"),
        }
        #[derive(serde::Deserialize)]
        struct Status {
            #[serde(rename = "child-pid")]
            child: i32,
        }
        let status: Status = match serde_json::from_slice(&self.bytes) {
            Ok(status) => status,
            Err(error) if error.is_eof() => return Ok(false),
            Err(error) => return Err(error).context("invalid sandbox launch status"),
        };
        let pid =
            rustix::process::Pid::from_raw(status.child).context("invalid sandbox init PID")?;
        let namespace = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::NONBLOCK)
            .context("pin sandbox namespace init")?;
        // Pin first. A recycled numeric PID cannot pass this relationship: this
        // bwrap child creates exactly one PID namespace init before the gate.
        let child = std::fs::read_to_string(format!("/proc/{}/status", status.child))?;
        let parent: u32 = field(&child, "PPid:")?.parse()?;
        ensure!(
            field(&child, "NSpid:")?.split_whitespace().last() == Some("1"),
            "sandbox owner is not namespace init"
        );
        let bwrap = std::fs::read_to_string(format!("/proc/{parent}/status"))?;
        ensure!(
            field(&bwrap, "PPid:")?.parse::<u32>()? == supervisor,
            "sandbox owner has a different supervisor"
        );
        self.namespace = Some(namespace);
        Ok(true)
    }
    pub fn admit(&mut self) -> Result<()> {
        ensure!(
            self.namespace.is_some() && !self.admitted,
            "sandbox owner must be pinned before launch"
        );
        self.gate
            .as_mut()
            .context("sandbox launch was revoked")?
            .write_all(b"1")?;
        self.gate.take();
        self.admitted = true;
        Ok(())
    }
    pub fn admitted(&self) -> bool {
        self.admitted
    }
    pub fn stop(&mut self) {
        self.gate.take();
        if let Some(namespace) = &self.namespace {
            let _ = rustix::process::pidfd_send_signal(namespace, rustix::process::Signal::KILL);
        }
    }
    pub fn stopped(&self) -> Result<bool> {
        let Some(namespace) = &self.namespace else {
            // With no token sent, EOF at the trusted wrapper cannot run payload.
            return Ok(true);
        };
        let mut fds = [rustix::event::PollFd::new(
            namespace,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )?;
        Ok(fds[0]
            .revents()
            .intersects(rustix::event::PollFlags::IN | rustix::event::PollFlags::HUP))
    }
}
fn field<'a>(status: &'a str, key: &str) -> Result<&'a str> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .map(str::trim)
        .context("sandbox process identity field is missing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree_access::{HookView, WorktreeAccess};
    use std::{collections::BTreeMap, process::Stdio, sync::atomic::AtomicBool, time::Duration};

    #[tokio::test]
    async fn owner_revocation_after_namespace_pin_before_token_never_executes_payload() {
        for supervisor_dies in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let pin = File::open(root.path()).unwrap();
            let access = WorktreeAccess::new(root.path(), &[]).unwrap();
            let argv = [
                "/bin/bash".into(),
                "-c".into(),
                "printf bad > payload".into(),
            ];
            let writes = [".".into()];
            let environment = BTreeMap::new();
            let cancelled = AtomicBool::new(false);
            let command = access
                .hook_command(
                    HookView {
                        live: &pin,
                        snapshot: &pin,
                        code: &pin,
                        workspace: root.path(),
                        writes: &writes,
                        cwd: root.path(),
                        argv: &argv,
                        environment: &environment,
                    },
                    &cancelled,
                )
                .unwrap();
            let mut launch = Launch::new().unwrap();
            let mut child = tokio::process::Command::new("/bin/bash")
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    "\"$@\" & wait",
                    "fixture-supervisor",
                    "/bin/bash",
                ])
                .args(command.launch_arguments(&launch.status_writer, &launch.gate_reader))
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                while !launch.prepare(child.id().unwrap()).unwrap() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            assert!(!launch.admitted());
            assert!(!root.path().join("payload").exists());
            if supervisor_dies {
                child.start_kill().unwrap();
            }
            launch.stop();
            tokio::time::timeout(Duration::from_secs(3), async {
                child.wait().await.unwrap();
                while !launch.stopped().unwrap() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            assert!(!root.path().join("payload").exists());
        }
    }
}
