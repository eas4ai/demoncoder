use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};

pub struct BackendProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
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
        let mut command = tokio::process::Command::new(binary);
        command
            .args(args)
            .current_dir(workspace)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
        let mut child = command.spawn().context("start backend executable")?;
        let stdin = child.stdin.take().context("backend stdin unavailable")?;
        let stdout = BufReader::new(child.stdout.take().context("backend stdout unavailable")?);
        Ok(Self {
            child,
            stdin,
            stdout,
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
        let mut line = Vec::new();
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
            if line.len() + count > 4 * 1024 * 1024 {
                bail!("backend response exceeds 4 MiB");
            }
            let complete = bytes[count - 1] == b'\n';
            line.extend_from_slice(&bytes[..count]);
            self.stdout.consume(count);
            if complete {
                return serde_json::from_slice(&line)
                    .map_err(|_| anyhow::anyhow!("invalid backend JSON response"));
            }
        }
    }

    pub async fn stop(&mut self) -> Result<()> {
        match self.child.try_wait().context("inspect backend process")? {
            Some(_) => Ok(()),
            None => {
                self.child.start_kill().context("stop backend process")?;
                tokio::time::timeout(Duration::from_secs(2), self.child.wait())
                    .await
                    .context("backend did not stop within two seconds")?
                    .context("reap backend process")?;
                Ok(())
            }
        }
    }
}
