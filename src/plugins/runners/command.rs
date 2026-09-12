//! Immutable host-selected command registration and bounded event framing.
use super::event::LimitedInput;
use super::{package::PackageMount, process, snapshot::SnapshotMount};
use crate::plugins::{
    Dialect, Package, SourceValidity,
    dispatch::{Declaration, HookInvocation, HookRunner, Registration},
    hook_types::{HandlerKind, HookDialect, HookEvent},
    profile::CompatibilityProfile,
    receipts::{DeclarationIdentity, HandlerClass, RawOutcome},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const CODE_ROOT: &str = "/__demoncoder_hook_code";
#[derive(Clone, Serialize)]
pub enum CommandProgram {
    Argv(Vec<String>),
    Shell(String),
}
#[derive(Clone, Serialize, PartialEq, Eq)]
pub enum NetworkGrant {
    None,
    Allowlist(Vec<String>),
    Host,
}

/// Explicit trusted host configuration. Importing a Package does not create it.
#[derive(Clone, Serialize)]
pub struct CommandConfig {
    pub asynchronous: bool,
    pub async_rewake: bool,
    pub program: CommandProgram,
    pub environment: BTreeMap<String, String>,
    /// Canonical relative cwd inside the workspace, or `.` for its root.
    pub cwd: String,
    pub write_paths: Vec<String>,
    pub network: NetworkGrant,
    pub timeout_ms: u64,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    /// Required source wire facts when selecting the Codex command dialect.
    pub model: Option<String>,
    pub permission_mode: String,
    pub transcript_path: Option<String>,
}
impl CommandConfig {
    pub fn new(program: CommandProgram) -> Self {
        Self {
            asynchronous: false,
            async_rewake: false,
            program,
            environment: BTreeMap::new(),
            cwd: ".".into(),
            write_paths: Vec::new(),
            network: NetworkGrant::None,
            timeout_ms: 10_000,
            max_input_bytes: 65_536,
            max_output_bytes: 28 * 1024,
            model: None,
            permission_mode: "default".into(),
            transcript_path: None,
        }
    }
    pub(crate) fn validate(&self, class: HandlerClass, dialect: HookDialect) -> Result<()> {
        ensure!(
            !self.async_rewake || dialect == HookDialect::Claude,
            "rewake requires Claude source semantics"
        );
        ensure!(
            self.network == NetworkGrant::None,
            "hook network grants beyond None are not integrated"
        );
        ensure!(
            (1..=20_000).contains(&self.timeout_ms),
            "hook timeout must be between 1 and 20000 ms"
        );
        ensure!(
            (1..=65536).contains(&self.max_input_bytes)
                && (1..=28 * 1024).contains(&self.max_output_bytes),
            "hook input or output bound is invalid"
        );
        let text = |s: &str| -> Result<()> {
            ensure!(
                s.len() <= 16384 && !s.contains('\0'),
                "hook configuration string exceeds bounds"
            );
            Ok(())
        };
        match &self.program {
            CommandProgram::Argv(args) => {
                ensure!(
                    !args.is_empty() && args.len() <= 128 && !args[0].is_empty(),
                    "hook argv requires 1 to 128 arguments"
                );
                for arg in args {
                    text(arg)?;
                }
            }
            CommandProgram::Shell(script) => {
                text(script)?;
                ensure!(!script.is_empty(), "hook shell source is empty");
            }
        }
        ensure!(
            self.environment.len() <= 64,
            "hook environment exceeds bounds"
        );
        for (key, value) in &self.environment {
            ensure!(
                !key.is_empty()
                    && key.len() <= 128
                    && key.bytes().enumerate().all(|(i, b)| b == b'_'
                        || b.is_ascii_alphabetic()
                        || (i > 0 && b.is_ascii_digit())),
                "invalid hook environment name"
            );
            ensure!(
                ![
                    "PATH",
                    "HOME",
                    "TMPDIR",
                    "LANG",
                    "CLAUDE_PLUGIN_ROOT",
                    "CODEX_PLUGIN_ROOT"
                ]
                .contains(&key.as_str()),
                "hook environment overrides a reserved host value"
            );
            text(value)?;
        }
        if self.cwd != "." {
            super::package::checked_path(&self.cwd)?;
        }
        ensure!(
            self.write_paths.len() <= 32
                && (class != HandlerClass::DecisionGate || self.write_paths.is_empty()),
            "decision gates cannot have workspace mutation grants"
        );
        for path in &self.write_paths {
            if path != "." {
                super::package::checked_path(path)?;
            }
            ensure!(
                !std::path::Path::new(path)
                    .components()
                    .any(|c| c.as_os_str() == ".git")
                    && !crate::export_policy::private_path(std::path::Path::new(path)),
                "hook mutation grant includes a protected path"
            );
        }
        for (i, path) in self.write_paths.iter().enumerate() {
            ensure!(
                !self.write_paths[..i].iter().any(|p| p == "."
                    || path == "."
                    || std::path::Path::new(p).starts_with(path)
                    || std::path::Path::new(path).starts_with(p)),
                "overlapping hook mutation grants"
            );
        }
        for value in self
            .model
            .iter()
            .chain(&self.transcript_path)
            .chain(std::iter::once(&self.permission_mode))
        {
            text(value)?;
        }
        if dialect == HookDialect::Codex {
            ensure!(
                self.model.as_ref().is_some_and(|v| !v.is_empty()),
                "Codex hook input requires the host model identity"
            );
        }
        ensure!(
            [
                "default",
                "acceptEdits",
                "plan",
                "dontAsk",
                "bypassPermissions"
            ]
            .contains(&self.permission_mode.as_str()),
            "invalid source permission mode"
        );
        // Each field is bounded before serialization, so this final aggregate
        // bound cannot allocate an attacker-sized configuration first.
        let mut encoded = LimitedInput {
            bytes: Vec::new(),
            maximum: 32768,
        };
        serde_json::to_writer(&mut encoded, self)
            .context("aggregate hook configuration exceeds bounds")?;
        Ok(())
    }
}

pub struct CommandRunner {
    required_gate: bool,
    event: HookEvent,
    package: Arc<Package>,
    identity: DeclarationIdentity,
    config: CommandConfig,
    class: HandlerClass,
    endpoint: Option<String>,
    profile: Arc<CompatibilityProfile>,
}
impl CommandRunner {
    pub fn registration(
        package: Arc<Package>,
        declaration: Declaration,
        config: CommandConfig,
        revalidation: Option<CommandConfig>,
    ) -> Result<Registration> {
        Self::registration_for_event(
            package,
            declaration,
            HookEvent::PreToolUse,
            config,
            revalidation,
        )
    }
    pub fn registration_for_event(
        package: Arc<Package>,
        mut declaration: Declaration,
        event: HookEvent,
        config: CommandConfig,
        revalidation: Option<CommandConfig>,
    ) -> Result<Registration> {
        ensure!(
            package.source_validity() == SourceValidity::Valid,
            "command package has invalid or unvalidated components"
        );
        ensure!(
            declaration.identity.runner == HandlerKind::Command,
            "command registration has a different runner kind"
        );
        let dialect = declaration.identity.dialect;
        ensure!(
            dialect == HookDialect::Native
                || matches!(
                    (dialect, package.dialect()),
                    (HookDialect::Claude, Dialect::Claude)
                        | (HookDialect::Codex, Dialect::Codex | Dialect::Portable)
                ),
            "command package and source dialect differ"
        );
        ensure!(
            !(config.asynchronous || config.async_rewake) || !declaration.required_gate,
            "async scheduling cannot satisfy a required gate"
        );
        config.validate(declaration.class, dialect)?;
        ensure!(
            declaration.read_only_endpoint.is_some() == revalidation.is_some(),
            "revalidation requires an explicit read-only command"
        );
        if let Some(config) = &revalidation {
            config.validate(HandlerClass::DecisionGate, dialect)?;
        }
        declaration.identity.package = package.name().to_owned();
        declaration.bind_package_source(&package)?;
        declaration.identity.code = package.digest().to_owned();
        declaration.identity.configuration =
            crate::plugins::admission::digest(&(event, &config, &revalidation))?;
        let profile = Arc::new(CompatibilityProfile::embedded()?);
        profile.require_runner(dialect, event, HandlerKind::Command)?;
        let runner = Arc::new(Self {
            required_gate: declaration.required_gate,
            event,
            package: package.clone(),
            identity: declaration.identity.clone(),
            config,
            class: declaration.class,
            endpoint: None,
            profile: profile.clone(),
        });
        let revalidation = revalidation.map(|config| {
            Arc::new(Self {
                required_gate: true,
                event,
                package,
                identity: declaration.identity.clone(),
                config,
                class: HandlerClass::DecisionGate,
                endpoint: declaration.read_only_endpoint.clone(),
                profile,
            }) as Arc<dyn HookRunner>
        });
        Ok(Registration {
            declaration,
            runner,
            revalidation,
        })
    }
    fn input(&self, invocation: &HookInvocation) -> Result<Vec<u8>> {
        super::event::input(
            invocation,
            &self.profile,
            super::event::EventInput {
                dialect: self.identity.dialect,
                maximum: self.config.max_input_bytes,
                model: self.config.model.as_deref(),
                permission_mode: &self.config.permission_mode,
                transcript_path: self.config.transcript_path.as_deref(),
            },
        )
    }
}
struct Cancellation(Arc<AtomicBool>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[async_trait::async_trait]
impl HookRunner for CommandRunner {
    fn observer_config(&self) -> Option<crate::plugins::observer::ObserverConfig> {
        (!self.required_gate
            && (self.config.asynchronous
                || self.config.async_rewake
                || self.identity.dialect == HookDialect::Claude))
            .then_some(crate::plugins::observer::ObserverConfig {
                declared: self.config.asynchronous,
                rewake: self.config.async_rewake,
                timeout_ms: self.config.timeout_ms,
            })
    }
    fn bound_event(&self) -> Option<HookEvent> {
        Some(self.event)
    }
    fn side_effect_free(&self) -> bool {
        self.config.write_paths.is_empty()
    }
    fn mutates_workspace(&self) -> bool {
        !self.config.write_paths.is_empty()
    }
    async fn run(&self, invocation: &HookInvocation) -> Result<RawOutcome> {
        let event = self.event;
        let checked = (|| -> Result<_> {
            ensure!(
                invocation.key.event == self.event.as_str()
                    && invocation.events.plugin_event() == self.event
                    && invocation.declaration == self.identity
                    && invocation.endpoint == self.endpoint
                    && invocation.class == self.class
                    && invocation.required_gate == self.required_gate,
                "command declaration/configuration identity mismatch"
            );
            let meta = invocation.host.root.metadata()?;
            use std::os::unix::fs::MetadataExt;
            ensure!(
                (meta.dev(), meta.ino()) == invocation.key.workspace
                    && invocation.snapshot.root_identity() == invocation.key.workspace,
                "command host workspace identity mismatch"
            );
            ensure!(
                self.config.write_paths.is_empty() || invocation.mutation_guard.is_some(),
                "command mutation boundary is unavailable"
            );
            ensure!(
                invocation.host.supervisor.is_some(),
                "command requires a configured runtime supervisor"
            );
            let input = self.input(invocation)?;
            let (runtime, operation) = invocation.events.plugin_context()?;
            if let Some(owner) = &invocation.observer {
                owner.validate()?;
            } else {
                runtime.plugin_runner_owner(operation, event)?;
            }
            // Leave normal cleanup headroom before run_owned's existing outer
            // deadline. A stuck kernel reap still retains all resources even if
            // that outer owner returns an unknown outcome; no late settlement.
            let available = if let Some(owner) = &invocation.observer {
                owner.validate()?
            } else {
                runtime
                    .plugin_remaining(operation, event)?
                    .min(Duration::from_secs(30))
                    .saturating_sub(Duration::from_secs(3))
            };
            ensure!(
                !available.is_zero(),
                "command deadline has no cleanup allowance"
            );
            Ok((
                input,
                runtime.downgrade(),
                operation,
                Instant::now() + available.min(Duration::from_millis(self.config.timeout_ms)),
            ))
        })();
        let (input, runtime, operation, deadline) = match checked {
            Ok(v) => v,
            Err(error) => return Ok(failure(&error.to_string())),
        };
        let cancellation = Cancellation(Arc::new(AtomicBool::new(false)));
        let cancelled = cancellation.0.clone();
        let package = self.package.clone();
        let snapshot = invocation.snapshot.clone();
        let host = invocation.host.clone();
        let config = self.config.clone();
        let lease = invocation.runner_lease.clone();
        let boundary = invocation.mutation_guard.clone();
        let observer = invocation.observer.clone();
        let first_line = self.identity.dialect == HookDialect::Claude;
        let outcome = tokio::task::spawn_blocking(move || {
            let _lease = lease;
            let _boundary = boundary;
            let validate = || -> Result<()> {
                if let Some(owner) = &observer {
                    owner.validate()?;
                } else {
                    let runtime = runtime.upgrade()?;
                    runtime.plugin_runner_owner(operation, event)?;
                    ensure!(
                        !runtime.plugin_remaining(operation, event)?.is_zero(),
                        "command owner expired"
                    );
                }
                Ok(())
            };
            let prepared = (|| -> Result<_> {
                // Apply the capture resolver again before any command sees retained
                // bytes. Keep frozen targets and add current aliases, including an
                // absent leaf under a valid directory alias; unresolved aliases hold.
                let mut credentials = host.credentials.clone();
                credentials.extend(
                    crate::plugins::gate_snapshot::protected_paths(
                        &host.workspace,
                        &host.credentials,
                    )?
                    .into_iter()
                    .map(|relative| host.workspace.join(relative)),
                );
                let access =
                    crate::worktree_access::WorktreeAccess::new(&host.workspace, &credentials)?;
                let snapshot = Arc::new(SnapshotMount::materialize(&snapshot, &cancelled)?);
                let code = PackageMount::materialize(&package, &cancelled)?;
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
                let cwd = host.workspace.join(&config.cwd);
                let command = access.hook_command(
                    crate::worktree_access::HookView {
                        live: &host.root,
                        snapshot: &snapshot.root,
                        code: &code.root,
                        workspace: &host.workspace,
                        writes: &config.write_paths,
                        cwd: &cwd,
                        argv: &argv,
                        environment: &environment,
                    },
                    &cancelled,
                )?;
                validate()?;
                ensure!(
                    !cancelled.load(Ordering::Acquire) && Instant::now() < deadline,
                    "command owner cancelled or expired before launch"
                );
                Ok((snapshot, code, access, command))
            })();
            match prepared {
                Ok((_snapshot, _code, _access, command)) => process::run(
                    &host,
                    &command,
                    &input,
                    config.max_output_bytes,
                    deadline,
                    &cancelled,
                    process::Owner {
                        validate: &validate,
                        first_line: first_line.then_some(
                            (&|marker| {
                                observer
                                    .as_ref()
                                    .context("async scheduling cannot satisfy a required gate")?
                                    .transfer(Some(marker))
                            })
                                as &dyn Fn(serde_json::Value) -> Result<()>,
                        ),
                    },
                ),
                Err(error) => failure(&error.to_string()),
            }
        })
        .await;
        Ok(outcome.unwrap_or_else(|_| failure("command owner failed; effects may be unknown")))
    }
}
pub(crate) fn expand_root(value: &str) -> String {
    value
        .replace("${CLAUDE_PLUGIN_ROOT}", CODE_ROOT)
        .replace("${CODEX_PLUGIN_ROOT}", CODE_ROOT)
}
pub(crate) fn environment(config: &CommandConfig) -> BTreeMap<String, String> {
    let mut environment: BTreeMap<_, _> = config
        .environment
        .iter()
        .map(|(k, v)| (k.clone(), expand_root(v)))
        .collect();
    environment.insert("CLAUDE_PLUGIN_ROOT".into(), CODE_ROOT.into());
    environment.insert("CODEX_PLUGIN_ROOT".into(), CODE_ROOT.into());
    environment
}
pub(super) fn failure(reason: &str) -> RawOutcome {
    RawOutcome::CommandFailure {
        reason: reason.chars().take(512).collect(),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}
