use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};

/// Complete newline-delimited JSON frame budget, including the newline.
pub(super) const RESPONSE_FRAME_LIMIT: usize = 4 * 1024 * 1024;

pub struct BackendProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    partial: Vec<u8>,
    last_response_bytes: usize,
}

pub fn executable(configured: Option<&Path>, default: &str) -> Result<PathBuf> {
    let path = match configured {
        Some(path) => path.to_path_buf(),
        None => std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(default))
            .find(|path| path.is_file())
            .with_context(|| format!("install {default} or configure its executable"))?,
    };
    path.canonicalize().context("resolve backend executable")
}

impl BackendProcess {
    pub fn spawn(
        binary: &Path,
        args: &[String],
        workspace: &Path,
        auth_env: &[&str],
    ) -> Result<Self> {
        Self::spawn_with_environment(binary, args, workspace, auth_env, &[])
    }

    pub fn spawn_with_environment(
        binary: &Path,
        args: &[String],
        workspace: &Path,
        auth_env: &[&str],
        environment: &[(&str, &str)],
    ) -> Result<Self> {
        Self::spawn_inner(binary, args, workspace, auth_env, environment, None)
    }

    pub fn spawn_supervised(
        binary: &Path,
        args: &[String],
        workspace: &Path,
        auth_env: &[&str],
        supervisor: &Path,
    ) -> Result<Self> {
        Self::spawn_inner(binary, args, workspace, auth_env, &[], Some(supervisor))
    }

    pub fn spawn_supervised_with_environment(
        binary: &Path,
        args: &[String],
        workspace: &Path,
        auth_env: &[&str],
        environment: &[(&str, &str)],
        supervisor: &Path,
    ) -> Result<Self> {
        Self::spawn_inner(
            binary,
            args,
            workspace,
            auth_env,
            environment,
            Some(supervisor),
        )
    }

    fn spawn_inner(
        binary: &Path,
        args: &[String],
        workspace: &Path,
        auth_env: &[&str],
        environment: &[(&str, &str)],
        supervisor: Option<&Path>,
    ) -> Result<Self> {
        let mut command = tokio::process::Command::new(supervisor.unwrap_or(binary));
        if supervisor.is_some() {
            command
                .arg("--supervise-backend")
                .arg(serde_json::to_string(&(binary, args, std::process::id()))?);
        } else {
            command.args(args);
        }
        command
            .current_dir(workspace)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        for name in [
            "PATH",
            "HOME",
            "USER",
            "LANG",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for name in auth_env {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.envs(environment.iter().copied());
        let leased_stdin = if supervisor.is_some() {
            let (reader, writer) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?;
            let lease = writer.try_clone()?;
            // stderr is a private write lease, never a diagnostic stream. Both
            // owners retain stdin until either can kill the entire group.
            command
                .stdin(Stdio::from(reader))
                .stderr(Stdio::from(lease));
            Some(ChildStdin::from_std(std::process::ChildStdin::from(
                writer,
            ))?)
        } else {
            None
        };
        let mut child = command.spawn().context("start backend executable")?;
        let stdin = match leased_stdin {
            Some(stdin) => stdin,
            None => child.stdin.take().context("backend stdin unavailable")?,
        };
        let stdout = BufReader::new(child.stdout.take().context("backend stdout unavailable")?);
        Ok(Self {
            child,
            stdin,
            stdout,
            partial: Vec::new(),
            last_response_bytes: 0,
        })
    }

    pub async fn send(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value).context("serialize backend request")?;
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(10), self.stdin.write_all(&bytes))
            .await
            .context("backend input timed out")?
            .context("write backend request")
    }

    pub async fn receive(&mut self) -> Result<Value> {
        self.receive_limited(RESPONSE_FRAME_LIMIT).await
    }

    pub async fn finite_json(&mut self, limit: usize) -> Result<Value> {
        let mut bytes = Vec::new();
        (&mut self.stdout)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .context("read backend status")?;
        anyhow::ensure!(bytes.len() <= limit, "backend status exceeds size limit");
        // Do not reap here: stop/Drop must still own the process-group identity.
        // EOF plus a valid status is not sufficient if the command failed.
        let pid = self
            .child
            .id()
            .and_then(|id| Pid::from_raw(id as i32))
            .context("backend process unavailable")?;
        loop {
            use rustix::process::{WaitId, WaitIdOptions, waitid};
            if let Some(status) = waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
            )
            .context("inspect backend status")?
            {
                anyhow::ensure!(
                    status.exit_status() == Some(0),
                    "backend login check failed; sign in with the backend CLI"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid backend status JSON"))
    }

    pub async fn receive_limited(&mut self, limit: usize) -> Result<Value> {
        // Steering can cancel this future between reads. Keep consumed bytes
        // on the connection so its next receive resumes the same JSON frame.
        loop {
            let bytes = self
                .stdout
                .fill_buf()
                .await
                .context("read backend response")?;
            if bytes.is_empty() {
                bail!("backend closed its response stream");
            }
            let count = bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |end| end + 1);
            if self.partial.len() + count > limit {
                bail!("backend response exceeds size limit");
            }
            let complete = bytes[count - 1] == b'\n';
            self.partial.extend_from_slice(&bytes[..count]);
            self.stdout.consume(count);
            if complete {
                self.last_response_bytes = self.partial.len();
                return serde_json::from_slice(&std::mem::take(&mut self.partial))
                    .map_err(|_| anyhow::anyhow!("invalid backend JSON response"));
            }
        }
    }

    pub fn last_response_bytes(&self) -> usize {
        self.last_response_bytes
    }

    /// Observe leader death without reaping its reserved process-group ID.
    pub async fn wait_for_exit(&self) -> Result<()> {
        use rustix::process::{WaitId, WaitIdOptions, waitid};
        let pid = self
            .child
            .id()
            .and_then(|id| Pid::from_raw(id as i32))
            .context("backend process unavailable")?;
        loop {
            if waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
            )?
            .is_some()
            {
                bail!("backend transport exited during lifecycle callback");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub async fn stop(&mut self) -> Result<()> {
        // Signal before reaping: the unreaped leader reserves this group ID,
        // including when it has exited while one of its helpers is still alive.
        self.kill_group().context("stop backend process group")?;
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .context("backend did not stop within two seconds")?
            .context("reap backend process")?;
        Ok(())
    }

    fn kill_group(&self) -> rustix::io::Result<()> {
        let Some(pid) = self.child.id().and_then(|id| Pid::from_raw(id as i32)) else {
            return Ok(());
        };
        match kill_process_group(pid, Signal::KILL) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Drop for BackendProcess {
    fn drop(&mut self) {
        // Emergency cleanup when an owning future is dropped. Normal close
        // reports errors through stop(); Drop cannot return a failure.
        let _ = self.kill_group();
    }
}

pub(crate) async fn stop_backend_and_services(
    process: &mut Option<BackendProcess>,
    services: impl std::future::Future<Output = Result<()>>,
) -> Result<()> {
    // A pending SDK callback must lose its backend owner before unrelated
    // cleanup can wait or fail. Drop also kills the group if stop fails.
    let backend = match process.take() {
        Some(mut process) => process.stop().await,
        None => Ok(()),
    };
    let services = services.await;
    match (backend, services) {
        (Err(backend), Err(services)) => {
            Err(backend.context(format!("service cleanup also failed: {services:#}")))
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_service_cleanup_cannot_leave_a_backend_alive() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let mut process = Some(BackendProcess::spawn(
            Path::new("/usr/bin/python3"),
            &[
                "-u".into(),
                "-c".into(),
                "import time; print('{}',flush=True); time.sleep(60)".into(),
            ],
            workspace.path(),
            &[],
        )?);
        process.as_mut().unwrap().receive().await?;
        let pid = process.as_ref().unwrap().child.id().unwrap();
        let result = stop_backend_and_services(&mut process, async {
            anyhow::ensure!(!running(pid), "backend survived into unrelated cleanup");
            bail!("injected service cleanup failure")
        })
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected service cleanup failure")
        );
        assert!(process.is_none());
        assert!(!running(pid));
        Ok(())
    }

    #[tokio::test]
    async fn finite_status_preserves_group_cleanup_and_rejects_failed_exit() -> Result<()> {
        for success in [true, false] {
            let workspace = tempfile::tempdir()?;
            let script = format!(
                "import json,subprocess; child=subprocess.Popen(['/usr/bin/sleep','60'],stdout=subprocess.DEVNULL); print(json.dumps({{'helper':child.pid}},indent=2),flush=True); raise SystemExit({})",
                if success { 0 } else { 1 }
            );
            let mut process = BackendProcess::spawn(
                Path::new("/usr/bin/python3"),
                &["-u".into(), "-c".into(), script],
                workspace.path(),
                &[],
            )?;
            let status =
                tokio::time::timeout(Duration::from_secs(3), process.finite_json(1024)).await?;
            assert_eq!(status.is_ok(), success);
            let helper = status
                .ok()
                .map(|value| value["helper"].as_u64().unwrap() as u32);
            process.stop().await?;
            if let Some(helper) = helper {
                tokio::time::timeout(Duration::from_secs(2), async {
                    while running(helper) {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .context("status helper survived cleanup")?;
            }
        }
        Ok(())
    }

    fn running(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(')')
                    .map(|(_, fields)| !fields.starts_with(" Z "))
            })
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn group_cleanup_covers_drop_and_exited_leader() -> Result<()> {
        for drop_owner in [false, true] {
            let workspace = tempfile::tempdir()?;
            let script = "import json,subprocess; child=subprocess.Popen(['/usr/bin/sleep','60']); print(json.dumps({'helper':child.pid}),flush=True)";
            let mut process = BackendProcess::spawn(
                Path::new("/usr/bin/python3"),
                &["-u".into(), "-c".into(), script.into()],
                workspace.path(),
                &[],
            )?;
            let helper = process.receive().await?["helper"].as_u64().unwrap() as u32;
            let leader = process.child.id().unwrap();
            assert!(running(helper));
            // Do not reap the exited leader: its PID still reserves our group.
            tokio::time::timeout(Duration::from_secs(2), async {
                while running(leader) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await?;
            if drop_owner {
                drop(process);
            } else {
                process.stop().await?;
                // A second close must not signal a stale numeric process ID.
                process.stop().await?;
            }
            tokio::time::timeout(Duration::from_secs(2), async {
                while running(helper) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .context("backend helper survived owner cleanup")?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_receive_retains_partial_json() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let script = "import sys; print('{}', flush=True); sys.stdout.write('{\"method\":'); sys.stdout.flush(); input(); print('\"retained\"}', flush=True)";
        let mut process = BackendProcess::spawn(
            Path::new("/usr/bin/python3"),
            &["-u".into(), "-c".into(), script.into()],
            workspace.path(),
            &[],
        )?;
        assert_eq!(process.receive().await?, serde_json::json!({}));
        assert!(
            tokio::time::timeout(Duration::from_secs(1), process.receive())
                .await
                .is_err()
        );
        assert_eq!(process.partial, b"{\"method\":");
        process.send(serde_json::json!({"release": true})).await?;
        assert_eq!(
            process.receive().await?,
            serde_json::json!({"method": "retained"})
        );
        process.stop().await
    }
}
