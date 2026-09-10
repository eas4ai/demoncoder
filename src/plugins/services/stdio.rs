//! A retained read-only service view and duplex pipe under the existing namespace owner.
use super::protocol::{self, Message};
use crate::{
    plugins::{
        Package,
        dispatch::HookInvocation,
        runners::{
            CommandConfig, CommandProgram,
            command::{environment, expand_root},
            launch::Launch,
            package::PackageMount,
            process::OwnedProcess,
            snapshot::SnapshotMount,
        },
    },
    workflow::runtime::{RuntimeReference, SharedRuntime, plugin_admission::ServiceOwner},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::{AsFd, AsRawFd},
    process::{ChildStderr, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) struct Pipe {
    // Process must stop before any retained mount/lease is released.
    process: OwnedProcess,
    input: Option<File>,
    output: ChildStdout,
    error: ChildStderr,
    _snapshot: SnapshotMount,
    _code: PackageMount,
    startup_lease: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
    uncertain_lease: Option<Arc<tokio::sync::OwnedSemaphorePermit>>,
    runtime: RuntimeReference,
    owner: ServiceOwner,
    revoked: Arc<AtomicBool>,
    pending: Vec<u8>,
    stderr_bytes: usize,
    secrets: protocol::Secrets,
}
pub(super) struct Start {
    runtime: SharedRuntime,
    owner: ServiceOwner,
    host: crate::plugins::runners::HookHost,
    snapshot: Arc<crate::plugins::gate_snapshot::GateSnapshot>,
    lease: Arc<tokio::sync::OwnedSemaphorePermit>,
}
impl Start {
    pub(super) fn capture(
        invocation: &HookInvocation,
        runtime: SharedRuntime,
        owner: ServiceOwner,
    ) -> Self {
        Self {
            runtime,
            owner,
            host: invocation.host.clone(),
            snapshot: invocation.snapshot.clone(),
            lease: invocation.runner_lease.clone(),
        }
    }
}
impl Pipe {
    pub(super) fn start(
        start: Start,
        package: Arc<Package>,
        config: CommandConfig,
        revoked: Arc<AtomicBool>,
        secrets: protocol::Secrets,
        deadline: Instant,
    ) -> Result<Self> {
        let runtime = start.runtime.clone();
        let owner = start.owner.clone();
        ensure!(
            !revoked.load(Ordering::Acquire),
            "MCP service revoked before launch"
        );
        runtime.validate_plugin_service(&owner)?;
        let mut credentials = start.host.credentials.clone();
        credentials.extend(
            crate::plugins::gate_snapshot::protected_paths(&start.host.workspace, &credentials)?
                .into_iter()
                .map(|p| start.host.workspace.join(p)),
        );
        let access =
            crate::worktree_access::WorktreeAccess::new(&start.host.workspace, &credentials)?;
        let snapshot = SnapshotMount::materialize(&start.snapshot, &revoked)?;
        let code = PackageMount::materialize(&package, &revoked)?;
        let environment = environment(&config);
        let argv = match &config.program {
            CommandProgram::Argv(args) => args.iter().map(|v| expand_root(v)).collect(),
            CommandProgram::Shell(script) => vec![
                "/bin/bash".into(),
                "--noprofile".into(),
                "--norc".into(),
                "-c".into(),
                script.clone(),
            ],
        };
        let command = access.hook_command(
            crate::worktree_access::HookView {
                live: &start.host.root,
                snapshot: &snapshot.root,
                code: &code.root,
                workspace: &start.host.workspace,
                writes: &[],
                cwd: &start.host.workspace.join(&config.cwd),
                argv: &argv,
                environment: &environment,
            },
            &revoked,
        )?;
        let launch = Launch::new()?;
        let (reader, writer) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?;
        let input_reader = File::from(reader);
        let input = File::from(writer);
        let encoded = serde_json::to_string(&crate::supervisor::HookLaunch {
            arguments: command.launch_arguments(&launch.status_writer, &launch.gate_reader),
            input: format!(
                "/proc/{}/fd/{}",
                std::process::id(),
                input_reader.as_raw_fd()
            ),
            duplex: true,
        })?;
        ensure!(
            encoded.len() <= 128 * 1024,
            "MCP launch description exceeds bound"
        );
        runtime.validate_plugin_service(&owner)?;
        ensure!(
            Instant::now() < deadline && !revoked.load(Ordering::Acquire),
            "MCP startup cancelled"
        );
        let child = Command::new(
            start
                .host
                .supervisor
                .as_ref()
                .context("MCP supervisor missing")?,
        )
        .args(["--supervise-hook", &encoded])
        .env_clear()
        .env("TOKIO_WORKER_THREADS", "1")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow::anyhow!("MCP supervisor launch failed"))?;
        let mut process = OwnedProcess {
            child,
            lease: None,
            status: None,
            launch,
        };
        process.lease = process.child.stdin.take();
        let output = process.child.stdout.take().context("MCP stdout missing")?;
        let error = process.child.stderr.take().context("MCP stderr missing")?;
        for fd in [&input as &dyn AsFd, &output, &error] {
            let flags = rustix::fs::fcntl_getfl(fd)?;
            rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)?;
        }
        process
            .lease
            .as_mut()
            .context("MCP lifetime lease missing")?
            .write_all(b"1")?;
        let mut pipe = Self {
            process,
            input: Some(input),
            output,
            error,
            _snapshot: snapshot,
            _code: code,
            startup_lease: Some(start.lease),
            uncertain_lease: None,
            runtime: runtime.downgrade(),
            owner,
            revoked,
            pending: Vec::new(),
            stderr_bytes: 0,
            secrets,
        };
        while !pipe.process.launch.admitted() {
            pipe.check(deadline)?;
            if pipe.process.launch.prepare(pipe.process.child.id())? {
                pipe.check(deadline)?;
                pipe.process.launch.admit()?;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(pipe)
    }
    pub(super) fn idle(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(20);
        self.check(deadline)?;
        let mut bytes = [0; 4096];
        match self.output.read(&mut bytes) {
            Ok(0) => anyhow::bail!("MCP idle stdout closed"),
            Ok(count) => {
                ensure!(
                    self.pending.len() + count <= protocol::MAX_MESSAGE,
                    "MCP idle frame exceeds bound"
                );
                self.pending.extend_from_slice(&bytes[..count]);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => anyhow::bail!("MCP idle transport failed"),
        }
        for _ in 0..protocol::MAX_MESSAGES {
            let Some(end) = self.pending.iter().position(|b| *b == b'\n') else {
                return Ok(());
            };
            let rest = self.pending.split_off(end + 1);
            let mut line = std::mem::replace(&mut self.pending, rest);
            line.pop();
            protocol::check_secrets(
                &line,
                &self
                    .secrets
                    .lock()
                    .map_err(|_| anyhow::anyhow!("MCP secret owner failed"))?,
            )?;
            match protocol::parse(&line, 0)? {
                Message::Result(_) => anyhow::bail!("MCP unsolicited idle response"),
                Message::Notification { tools_changed } => {
                    ensure!(!tools_changed, "MCP idle catalog changed")
                }
                Message::Request { id, ping } => {
                    self.write(&protocol::reply(id, ping), deadline)?;
                    ensure!(ping, "MCP idle request requires an unadvertised capability");
                }
            }
        }
        anyhow::bail!("MCP idle message count exceeds bound")
    }
    pub(super) fn release_startup(&mut self) {
        self.startup_lease.take();
    }
    pub(super) fn hold_uncertain(&mut self, lease: Option<Arc<tokio::sync::OwnedSemaphorePermit>>) {
        self.uncertain_lease = lease;
    }
    fn check(&mut self, deadline: Instant) -> Result<()> {
        ensure!(
            !self.revoked.load(Ordering::Acquire) && Instant::now() < deadline,
            "MCP service stopped or expired; effects may be unknown"
        );
        self.runtime
            .upgrade()?
            .validate_plugin_service(&self.owner)?;
        self.process.observe()?;
        ensure!(
            self.process.status.is_none(),
            "MCP process exited; effects may be unknown"
        );
        let mut bytes = [0; 4096];
        match self.error.read(&mut bytes) {
            Ok(count) => {
                self.stderr_bytes = self.stderr_bytes.saturating_add(count);
                ensure!(
                    self.stderr_bytes <= protocol::MAX_MESSAGE,
                    "MCP stderr exceeds bound"
                );
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => anyhow::bail!("MCP stderr transport failed"),
        }
        // Stderr is counted and discarded, never retained as result or diagnostic.
        Ok(())
    }
    fn write(&mut self, message: &Value, deadline: Instant) -> Result<()> {
        let mut bytes = protocol::encode(message)?;
        bytes.push(b'\n');
        let mut at = 0;
        while at < bytes.len() {
            self.check(deadline)?;
            match self
                .input
                .as_mut()
                .context("MCP input closed")?
                .write(&bytes[at..])
            {
                Ok(0) => anyhow::bail!("MCP input closed"),
                Ok(n) => at += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => anyhow::bail!("MCP write failed; effects may be unknown"),
            }
        }
        Ok(())
    }
    fn line(&mut self, deadline: Instant) -> Result<Vec<u8>> {
        loop {
            self.check(deadline)?;
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let rest = self.pending.split_off(end + 1);
                let mut line = std::mem::replace(&mut self.pending, rest);
                line.pop();
                return Ok(line);
            }
            let mut bytes = [0; 4096];
            match self.output.read(&mut bytes) {
                Ok(0) => anyhow::bail!("MCP stdout closed; effects may be unknown"),
                Ok(n) => {
                    ensure!(
                        self.pending.len() + n <= protocol::MAX_MESSAGE,
                        "MCP stdio frame exceeds bound"
                    );
                    self.pending.extend_from_slice(&bytes[..n]);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => anyhow::bail!("MCP stdio read failed"),
            }
        }
    }
    pub(super) fn send(
        &mut self,
        message: &Value,
        expected: Option<u64>,
        deadline: Instant,
    ) -> Result<Value> {
        self.idle()?;
        ensure!(
            self.pending.is_empty(),
            "MCP incomplete unsolicited message before request"
        );
        self.write(message, deadline)?;
        let Some(id) = expected else {
            return Ok(serde_json::json!({}));
        };
        let mut total = 0;
        for messages in 0..protocol::MAX_MESSAGES {
            let line = self.line(deadline)?;
            total += line.len();
            ensure!(
                total <= protocol::MAX_MESSAGE * 4,
                "MCP aggregate stdio response exceeds bound"
            );
            protocol::check_secrets(
                &line,
                &self
                    .secrets
                    .lock()
                    .map_err(|_| anyhow::anyhow!("MCP secret owner failed"))?,
            )?;
            match protocol::parse(&line, id)? {
                Message::Result(value) => {
                    self.settle_received(deadline, messages + 1, total)?;
                    return Ok(value);
                }
                Message::Notification { tools_changed } => {
                    ensure!(!tools_changed, "MCP catalog changed; readmission required")
                }
                Message::Request { id, ping } => {
                    self.write(&protocol::reply(id, ping), deadline)?;
                    ensure!(ping, "MCP server requested an unadvertised host capability");
                }
            }
        }
        anyhow::bail!("MCP message count exceeds bound")
    }
    /// Drain only bytes available now. Later messages belong to the idle monitor;
    /// already-buffered ambiguity must never release the guarded operation.
    fn settle_received(
        &mut self,
        deadline: Instant,
        mut messages: usize,
        mut total: usize,
    ) -> Result<()> {
        loop {
            self.check(deadline)?;
            while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                messages += 1;
                total = total.saturating_add(end + 1);
                ensure!(
                    messages <= protocol::MAX_MESSAGES && total <= protocol::MAX_MESSAGE * 4,
                    "MCP response settlement exceeds bound"
                );
                let rest = self.pending.split_off(end + 1);
                let mut line = std::mem::replace(&mut self.pending, rest);
                line.pop();
                protocol::check_secrets(
                    &line,
                    &self
                        .secrets
                        .lock()
                        .map_err(|_| anyhow::anyhow!("MCP secret owner failed"))?,
                )?;
                match protocol::parse(&line, 0)? {
                    Message::Result(_) => anyhow::bail!("MCP duplicate or unsolicited response"),
                    Message::Notification { tools_changed } => {
                        ensure!(!tools_changed, "MCP catalog changed during response")
                    }
                    Message::Request { id, ping } => {
                        self.write(&protocol::reply(id, ping), deadline)?;
                        ensure!(ping, "MCP server requested an unadvertised host capability");
                    }
                }
            }
            let mut bytes = [0; 4096];
            match self.output.read(&mut bytes) {
                Ok(0) => anyhow::bail!("MCP stdout closed during response settlement"),
                Ok(count) => {
                    ensure!(
                        self.pending.len() + count <= protocol::MAX_MESSAGE
                            && total
                                .saturating_add(self.pending.len())
                                .saturating_add(count)
                                <= protocol::MAX_MESSAGE * 4,
                        "MCP response settlement exceeds bound"
                    );
                    self.pending.extend_from_slice(&bytes[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    ensure!(
                        self.pending.is_empty(),
                        "MCP incomplete framing after response"
                    );
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => anyhow::bail!("MCP response settlement transport failed"),
            }
        }
    }
    pub(super) fn close(&mut self) -> Result<()> {
        self.input.take();
        let deadline = Instant::now() + Duration::from_millis(100);
        while Instant::now() < deadline {
            self.process.observe()?;
            if self.process.status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.process.revoke();
        // OwnedProcess retains mounts/lease until exact descendant shutdown is observed.
        Ok(())
    }
}
