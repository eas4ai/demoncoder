//! Optional, workspace-scoped language services owned by the shared tool boundary.
mod protocol;
mod view;

use crate::developer_access::DeveloperAccess;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    sync::Mutex,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const DIAGNOSTIC_WAIT: Duration = Duration::from_secs(2);
const MAX_DOCUMENTS: usize = 32;
const MAX_MESSAGES: usize = 1024;

#[derive(Clone, Default, Debug)]
pub struct LanguageServers {
    pub rust: Option<PathBuf>,
    pub typescript: Option<PathBuf>,
    pub read_roots: Vec<PathBuf>,
}

impl LanguageServers {
    pub(crate) fn enabled(&self) -> bool {
        self.rust.is_some() || self.typescript.is_some()
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.read_roots.len() <= 32,
            "at most 32 language read roots may be selected"
        );
        for path in [&self.rust, &self.typescript]
            .into_iter()
            .flatten()
            .chain(self.read_roots.iter())
        {
            ensure!(
                path.is_absolute(),
                "language server executable must be an absolute path"
            );
            ensure!(
                path.as_os_str().len() <= 4096,
                "language server executable path is too long"
            );
        }
        Ok(())
    }
    fn executable(&self, language: Language) -> Option<&Path> {
        match language {
            Language::Rust => self.rust.as_deref(),
            Language::Typescript => self.typescript.as_deref(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
enum Language {
    Rust,
    Typescript,
}

impl Language {
    fn for_path(path: &Path) -> Result<Self> {
        match path.extension().and_then(|s| s.to_str()) {
            Some("rs") => Ok(Self::Rust),
            Some("ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs") => {
                Ok(Self::Typescript)
            }
            _ => bail!("language service unavailable for this file type"),
        }
    }
    fn document_id(self, path: &Path) -> &'static str {
        match (self, path.extension().and_then(|s| s.to_str())) {
            (Self::Rust, _) => "rust",
            (_, Some("tsx")) => "typescriptreact",
            (_, Some("jsx")) => "javascriptreact",
            (_, Some("js" | "mjs" | "cjs")) => "javascript",
            _ => "typescript",
        }
    }
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Operation {
    Status,
    Definition,
    References,
    Hover,
    Diagnostics,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    operation: Operation,
    pub(crate) path: Option<String>,
    language: Option<Language>,
    line: Option<u32>,
    character: Option<u32>,
    #[serde(skip)]
    saved: bool,
}

impl Args {
    pub(crate) fn diagnostics(path: &str) -> Self {
        Self {
            operation: Operation::Diagnostics,
            path: Some(path.into()),
            language: None,
            line: None,
            character: None,
            saved: true,
        }
    }
    pub(crate) fn is_status(&self) -> bool {
        self.operation == Operation::Status
    }
}

pub(crate) fn definition() -> Value {
    json!({"name":"lsp","description":"Query explicitly enabled Rust/TypeScript language servers. Operations: status, definition, references, hover, diagnostics. Paths must be relative workspace source files. Positions are zero-based UTF-16 code units. Results identify source revisions; unknown/pending diagnostics do not mean clean. No rename, edits or commands are applied. Initial filtered-view admission is bounded to 150 seconds; each server request is bounded to 20 seconds. Admission and server errors are explicit.","input_schema":{"type":"object","properties":{"operation":{"type":"string","enum":["status","definition","references","hover","diagnostics"]},"path":{"type":"string"},"language":{"type":"string","enum":["rust","typescript"]},"line":{"type":"integer","minimum":0},"character":{"type":"integer","minimum":0}},"required":["operation"],"additionalProperties":false}})
}

pub(crate) struct Manager {
    config: LanguageServers,
    workspace: PathBuf,
    root: Arc<File>,
    access: Arc<DeveloperAccess>,
    state: Arc<Mutex<State>>,
    changes: Arc<std::sync::Mutex<Changes>>,
    observer: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Manager {
    pub(crate) fn watches(&self, path: &str) -> bool {
        Language::for_path(Path::new(path))
            .is_ok_and(|language| self.config.executable(language).is_some())
    }

    pub(crate) async fn stop(&mut self) -> Result<()> {
        if let Some(observer) = self
            .observer
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            observer.abort();
        }
        self.state.lock().await.stop().await
    }

    pub(crate) fn new(
        config: LanguageServers,
        workspace: PathBuf,
        root: Arc<File>,
        access: Arc<DeveloperAccess>,
    ) -> Self {
        let initial = State::default();
        let changes = initial.changes.clone();
        let state = Arc::new(Mutex::new(initial));
        Self {
            config,
            workspace,
            root,
            access,
            state,
            changes,
            observer: std::sync::Mutex::new(None),
        }
    }

    fn start_observer(&self) -> Result<()> {
        let mut observer = self
            .observer
            .lock()
            .map_err(|_| anyhow::anyhow!("language observer lock failed"))?;
        if observer.is_none() {
            *observer = Some(observe(
                self.config.clone(),
                self.workspace.clone(),
                self.root.clone(),
                self.access.clone(),
                Arc::downgrade(&self.state),
                self.changes.clone(),
            ));
        }
        Ok(())
    }

    pub(crate) async fn execute(&self, args: Args, source: Option<String>) -> Result<String> {
        self.start_observer()?;
        let result = tokio::time::timeout(Duration::from_secs(150), self.query(args, source)).await
            .context("language view or startup exceeded 150 seconds; select smaller runtime roots and retry")??;
        let output = serde_json::to_string(&result)?;
        ensure!(
            output.len() <= 512 * 1024,
            "language result exceeds 512 KiB; narrow the query"
        );
        Ok(output)
    }

    async fn query(&self, args: Args, source: Option<String>) -> Result<Value> {
        let language = if let Some(path) = &args.path {
            let inferred = Language::for_path(Path::new(path))?;
            ensure!(
                args.language.is_none_or(|v| v == inferred),
                "language does not match source file"
            );
            inferred
        } else {
            args.language
                .context("specify language for status, or a source path for other operations")?
        };
        let binary = self
            .config
            .executable(language)
            .context("language server is disabled; enable its executable at launch")?;
        ensure!(
            binary.is_file(),
            "language server executable is unavailable: {}",
            binary.display()
        );
        // Taking the server out before awaiting gives this request sole ownership.
        // Dropping a cancelled request drops the child; no stale protocol response
        // can leak into a later request. The idle map only holds completed servers.
        let mut state = self.state.lock().await;
        if let Some(path) = &args.path {
            let absolute = self.workspace.join(path);
            if !state.explicit.contains(&absolute) {
                ensure!(
                    state.explicit.len() < MAX_DOCUMENTS,
                    "language explicit source admission exceeds 32 files; reopen session"
                );
                state.explicit.push(absolute);
            }
        }
        state
            .refresh(&self.config, &self.workspace, &self.root, &self.access)
            .await?;
        if let Some(path) = &args.path {
            ensure!(
                state
                    .view
                    .as_ref()
                    .is_some_and(|view| view.manifest.contains_key(&self.workspace.join(path))),
                "language source was excluded by admission policy"
            );
        }
        let mut server = match state.servers.remove(&language) {
            Some(server) => server,
            None => {
                Server::start(
                    language,
                    binary,
                    &self.workspace,
                    state.view.as_ref().context("language view unavailable")?,
                    state.next_version,
                )
                .await?
            }
        };
        ensure!(
            server._child.try_wait()?.is_none(),
            "language server exited; query again to initialize a new server"
        );
        let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let result = if args.operation == Operation::Status {
            ensure!(
                args.line.is_none() && args.character.is_none(),
                "status does not accept a position"
            );
            json!({"state":server.project_state(),"server_status":server.status,"project_state":if server.status.is_some() {server.project_state()} else {"unknown"},"language":language,"workspace":self.workspace,"capabilities":server.supported(),"position_encoding":"utf-16","filesystem":"filtered independent project copies with a disposable private write layer; read-only selected runtimes","admission":{"state":"current","denied_cache_entries":state.denials.count(),"source_limit_mib":128,"runtime_limit_mib":4096,"startup_limit_seconds":150,"observer_error":state.observer_error,"refresh_error":state.refresh_error}})
        } else {
            let path = args.path.as_deref().context("source path is required")?;
            let text = source.context("source text is required")?;
            let uri = file_uri(&self.workspace.join(path))?;
            server
                .synchronize(
                    &uri,
                    language.document_id(Path::new(path)),
                    &text,
                    args.saved,
                )
                .await?;
            let document = server
                .documents
                .get(&uri)
                .context("document was not synchronized")?;
            let revision = json!({"path":path,"version":document.version,"sha256":document.digest});
            let data = match args.operation {
                Operation::Diagnostics => {
                    ensure!(
                        args.line.is_none() && args.character.is_none(),
                        "diagnostics does not accept a position"
                    );
                    server.diagnostics(&uri).await?
                }
                Operation::Definition | Operation::References | Operation::Hover => {
                    let line = args.line.context("line is required")?;
                    let character = args.character.context("character is required")?;
                    validate_position(&text, line, character)?;
                    let (method, capability) = match args.operation {
                        Operation::Definition => ("textDocument/definition", "definitionProvider"),
                        Operation::References => ("textDocument/references", "referencesProvider"),
                        _ => ("textDocument/hover", "hoverProvider"),
                    };
                    ensure!(
                        server.capable(capability),
                        "language server does not support {method}"
                    );
                    let mut params = json!({"textDocument":{"uri":uri},"position":{"line":line,"character":character}});
                    if args.operation == Operation::References {
                        params["context"] = json!({"includeDeclaration":true});
                    }
                    let value = server.request(method, params).await?;
                    validate_result_uris(&value, &self.workspace)?;
                    json!({"state":if server.project_state() == "ready" {"available"} else {server.project_state()},"result":value})
                }
                Operation::Status => unreachable!(),
            };
            let mut data = data;
            if server.project_state() != "ready" {
                data["state"] = json!(server.project_state());
                data["server_status"] = server.status.clone().unwrap_or(Value::Null);
            }
            json!({"language":language,"source":revision,"data":data,"truncated":false})
        };
        Ok::<Value, anyhow::Error>(result)
        }).await.context("language server exceeded its 20 second request limit; server stopped")?;
        // A completely received ContentModified response leaves framing intact.
        // Retain that peer after the retry allowance so indexing can progress.
        let result = match result {
            Ok(value) => value,
            Err(error) if error.is::<Pending>() => {
                state.next_version = state.next_version.max(server.next_version);
                state.servers.insert(language, server);
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        // Events are hints, not authority. Recheck the actual admitted input and
        // policy metadata before releasing a response from this cached peer.
        let current = state.inputs_current(&self.workspace, &self.root).await;
        if !current.as_ref().is_ok_and(|current| *current) {
            state.stop().await?;
            current?;
            bail!(
                "language inputs or admission policy changed during the request; retry for a current result"
            );
        }
        state.next_version = state.next_version.max(server.next_version);
        state.servers.insert(language, server);
        Ok(result)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if let Some(observer) = self
            .observer
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            observer.abort();
        }
    }
}

#[derive(Default)]
struct Changes {
    paths: BTreeSet<PathBuf>,
    unknown: bool,
}
impl Changes {
    fn record(&mut self, event: &notify::Result<notify::Event>) {
        match event {
            Ok(event) => {
                self.unknown |= event.need_rescan()
                    || matches!(
                        event.kind,
                        notify::EventKind::Any | notify::EventKind::Other
                    );
                for path in &event.paths {
                    if self.paths.len() >= 256 {
                        self.unknown = true;
                        break;
                    }
                    self.paths.insert(path.clone());
                }
            }
            Err(_) => self.unknown = true,
        }
    }
    fn affects(&self, view: &view::View) -> bool {
        self.unknown
            || self.paths.iter().any(|path| {
                path.file_name().is_some_and(|name| name == ".gitignore")
                    || view
                        .manifest
                        .range(path.clone()..)
                        .next()
                        .is_some_and(|(entry, _)| entry.starts_with(path))
            })
    }
}

struct State {
    servers: BTreeMap<Language, Server>,
    view: Option<Arc<view::View>>,
    denials: view::Denials,
    explicit: Vec<PathBuf>,
    next_version: i32,
    observer_error: Option<String>,
    admission_started: bool,
    refresh_error: Option<String>,
    changes: Arc<std::sync::Mutex<Changes>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            servers: BTreeMap::new(),
            view: None,
            denials: view::Denials::default(),
            explicit: Vec::new(),
            next_version: 1,
            observer_error: None,
            admission_started: false,
            refresh_error: None,
            changes: Arc::new(std::sync::Mutex::new(Changes::default())),
        }
    }
}
impl State {
    async fn stop(&mut self) -> Result<()> {
        for (_, mut server) in std::mem::take(&mut self.servers) {
            self.next_version = self.next_version.max(server.next_version);
            if server._child.try_wait()?.is_none() {
                server._child.start_kill().context("stop language server")?;
                tokio::time::timeout(Duration::from_secs(2), server._child.wait())
                    .await
                    .context("language server did not stop within two seconds")??;
            }
        }
        Ok(())
    }
    fn take_changes(&self) -> Changes {
        std::mem::take(
            &mut *self
                .changes
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }

    async fn inputs_current(&self, workspace: &Path, root: &Arc<File>) -> Result<bool> {
        let Some(view) = self.view.clone() else {
            return Ok(false);
        };
        let workspace = workspace.to_owned();
        let root = root.clone();
        crate::developer_access::inspect(move |cancelled| {
            view.inputs_current(&workspace, &root, cancelled)
        })
        .await
    }

    async fn refresh(
        &mut self,
        config: &LanguageServers,
        workspace: &Path,
        root: &Arc<File>,
        access: &Arc<DeveloperAccess>,
    ) -> Result<()> {
        self.admission_started = true;
        let changes = self.take_changes();
        if self.view.as_ref().is_some_and(|view| changes.affects(view)) {
            self.stop().await?;
        }
        if self.view.is_some() {
            let current = self.inputs_current(workspace, root).await;
            if !current.as_ref().is_ok_and(|current| *current) {
                self.stop().await?;
                current?;
            }
        }
        let config = config.clone();
        let workspace = workspace.to_owned();
        let root = root.clone();
        let private = access.language_private_roots().to_vec();
        let explicit = self.explicit.clone();
        let mut denials = std::mem::take(&mut self.denials);
        let previous = self.view.clone();
        let result = crate::developer_access::inspect(move |cancelled| {
            let result = view::View::build(
                &config,
                view::Source {
                    path: &workspace,
                    root: &root,
                },
                &private,
                &explicit,
                &mut denials,
                cancelled,
                previous.as_deref(),
            );
            Ok((result, denials))
        })
        .await?;
        self.denials = result.1;
        match result.0 {
            Ok(candidate) => {
                self.refresh_error = None;
                if self
                    .view
                    .as_ref()
                    .is_none_or(|view| !view.same_revision(&candidate))
                {
                    self.stop().await?;
                    self.view = Some(Arc::new(candidate));
                }
                Ok(())
            }
            Err(error) => {
                self.stop().await?;
                self.view = None;
                self.refresh_error =
                    Some(format!("language filesystem refresh unavailable: {error}"));
                Err(error)
            }
        }
    }
}

fn observe(
    config: LanguageServers,
    workspace: PathBuf,
    root: Arc<File>,
    access: Arc<DeveloperAccess>,
    state: std::sync::Weak<Mutex<State>>,
    changes: Arc<std::sync::Mutex<Changes>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use notify::Watcher;
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        // Event paths are hints only. A one-slot queue coalesces overflow into a
        // full reconciliation, retaining no unbounded attacker-controlled names.
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if !event
                .as_ref()
                .is_ok_and(|event| matches!(event.kind, notify::EventKind::Access(_)))
            {
                changes
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .record(&event);
                let _ = tx.try_send(());
            }
        });
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                if let Some(state) = state.upgrade() {
                    state.lock().await.observer_error =
                        Some(format!("filesystem observer unavailable: {error}"));
                }
                return;
            }
        };
        for path in std::iter::once(&workspace)
            .chain(config.read_roots.iter())
            .chain([&config.rust, &config.typescript].into_iter().flatten())
        {
            if let Err(error) = watcher.watch(path, notify::RecursiveMode::Recursive)
                && let Some(state) = state.upgrade()
            {
                state.lock().await.observer_error =
                    Some(format!("filesystem observation unavailable: {error}"));
            }
        }
        while rx.recv().await.is_some() {
            if !debounce(&mut rx).await {
                return;
            }
            let Some(state) = state.upgrade() else {
                return;
            };
            let mut state = state.lock().await;
            // Lazy initialization: observation maintains an existing session,
            // but enabling a server alone does not start expensive copying.
            if state.admission_started
                && let Err(error) = state.refresh(&config, &workspace, &root, &access).await
            {
                state.refresh_error =
                    Some(format!("language filesystem refresh unavailable: {error}"));
            }
        }
    })
}

/// The trailing timer runs even when the batch ends with one event. The fixed
/// deadline prevents a busy producer from postponing reconciliation forever.
async fn debounce(rx: &mut tokio::sync::mpsc::Receiver<()>) -> bool {
    let maximum = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        let trailing = tokio::time::Instant::now() + Duration::from_millis(200);
        tokio::select! {
            _ = tokio::time::sleep_until(trailing.min(maximum)) => return true,
            event = rx.recv() => {
                if event.is_none() { return false; }
                if tokio::time::Instant::now() >= maximum { return true; }
            }
        }
    }
}

fn file_uri(path: &Path) -> Result<String> {
    Ok(reqwest::Url::from_file_path(path)
        .map_err(|()| anyhow::anyhow!("invalid source path"))?
        .to_string())
}

fn validate_position(text: &str, line: u32, character: u32) -> Result<()> {
    let line = text
        .split('\n')
        .nth(line as usize)
        .context("line is outside the current source")?
        .trim_end_matches('\r');
    let mut offset = 0u32;
    for c in line.chars() {
        if offset == character {
            return Ok(());
        }
        offset += c.len_utf16() as u32;
        ensure!(
            offset <= character,
            "character splits a UTF-16 surrogate pair"
        );
    }
    ensure!(
        offset == character,
        "character is outside the current source line"
    );
    Ok(())
}

fn validate_result_uris(value: &Value, workspace: &Path) -> Result<()> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(key.as_str(), "uri" | "targetUri") {
                    let uri = reqwest::Url::parse(
                        value.as_str().context("invalid language result URI")?,
                    )?;
                    let path = uri
                        .to_file_path()
                        .map_err(|()| anyhow::anyhow!("language result contains a non-file URI"))?;
                    let resolved = path
                        .canonicalize()
                        .context("language result target is unavailable")?;
                    ensure!(
                        resolved.starts_with(workspace),
                        "language result target is outside the selected workspace"
                    );
                    ensure!(
                        !crate::export_policy::private_path(&resolved),
                        "language result targets a protected file"
                    );
                }
                validate_result_uris(value, workspace)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_result_uris(value, workspace)?;
            }
        }
        _ => {}
    }
    Ok(())
}

struct Document {
    version: i32,
    digest: String,
    diagnostics: Option<Value>,
    diagnostic_version: Option<i32>,
    versioned_diagnostics: Option<Value>,
}

impl Document {
    fn diagnostic_report(&self, pulled: Option<&[Value]>) -> Result<Value> {
        let pushed = self.diagnostics.as_ref().and_then(Value::as_array);
        ensure!(
            pulled.is_some() || pushed.is_some(),
            "diagnostics pending: no report received"
        );
        let mut seen = BTreeSet::new();
        let mut items = Vec::new();
        for item in pulled
            .into_iter()
            .flatten()
            .chain(pushed.into_iter().flatten())
        {
            if seen.insert(serde_json::to_string(item)?) {
                items.push(item.clone());
            }
        }
        // A full pull report does not version a separately pushed compiler
        // report. Preserve those errors, and keep uncertain freshness explicit.
        let current = pushed.is_none() || self.diagnostic_version == Some(self.version);
        let mut result = json!({"state":if current {"current"} else {"freshness_unknown"},"items":items,"verification":"not run"});
        if !current && let Some(known) = &self.versioned_diagnostics {
            result["last_versioned_items"] = known.clone();
        }
        Ok(result)
    }
}

struct Server {
    _child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    capabilities: Value,
    next_id: u64,
    documents: BTreeMap<String, Document>,
    next_version: i32,
    status: Option<Value>,
}

#[derive(Debug)]
enum Pending {
    ContentModified,
    ServerCancelled,
    Diagnostics,
}
impl std::fmt::Display for Pending {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ContentModified => "language project is pending: content modified during the request; retry when indexing settles",
            Self::ServerCancelled => "language request is pending: server cancelled and requested a retry",
            Self::Diagnostics => "diagnostics pending: no response within two seconds; retry while the server indexes",
        })
    }
}
impl std::error::Error for Pending {}

impl Server {
    async fn start(
        language: Language,
        binary: &Path,
        workspace: &Path,
        view: &view::View,
        next_version: i32,
    ) -> Result<Self> {
        ensure!(
            binary.is_absolute() && binary.is_file(),
            "language server executable is unavailable: {}",
            binary.display()
        );
        let path = binary
            .to_str()
            .context("language server executable path must be UTF-8")?;
        let script = format!(
            "exec {}'{}'{}",
            if language == Language::Rust {
                // Machine compiler-cache wrappers can require host sockets.
                // Language diagnostics use rustc directly within confinement;
                // ordinary verification keeps its separately declared command.
                "/usr/bin/env RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= "
            } else {
                ""
            },
            path.replace('\'', "'\\''"),
            if language == Language::Typescript {
                " --stdio"
            } else {
                ""
            }
        );
        let mut command = view.command(workspace, &script);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("start confined language server; no host fallback")?;
        let input = child.stdin.take().context("language server has no stdin")?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .context("language server has no stdout")?,
        );
        let mut server = Self {
            _child: child,
            input,
            output,
            capabilities: Value::Null,
            next_id: 1,
            documents: BTreeMap::new(),
            next_version,
            status: None,
        };
        let uri = file_uri(workspace)?;
        let initialized = server.request("initialize", json!({"processId":null,"rootUri":uri,"workspaceFolders":[{"uri":uri,"name":"workspace"}],"capabilities":{"experimental":{"serverStatusNotification":true},"general":{"positionEncodings":["utf-16"]},"workspace":{"applyEdit":false,"configuration":true,"workspaceFolders":false},"textDocument":{"publishDiagnostics":{"versionSupport":true},"synchronization":{"dynamicRegistration":false},"diagnostic":{"dynamicRegistration":false}}},"initializationOptions":null})).await.context("initialize confined language server; bubblewrap private-copy overlay support and selected interpreter/toolchain read roots are required; no host fallback")?;
        server.capabilities = initialized
            .get("capabilities")
            .filter(|v| v.is_object())
            .context("language server omitted capabilities")?
            .clone();
        ensure!(
            server
                .capabilities
                .get("positionEncoding")
                .is_none_or(|v| v == "utf-16"),
            "language server selected unsupported position encoding"
        );
        server.notify("initialized", json!({})).await?;
        Ok(server)
    }

    fn capable(&self, capability: &str) -> bool {
        self.capabilities
            .get(capability)
            .is_some_and(|v| v == true || v.is_object())
    }

    fn supported(&self) -> Value {
        let synchronized = self.synchronizes();
        json!({"definition":synchronized && self.capable("definitionProvider"),"references":synchronized && self.capable("referencesProvider"),"hover":synchronized && self.capable("hoverProvider"),"diagnostics":if synchronized {"push availability unknown until a notification; pull when advertised"} else {"unavailable: no document synchronization"},"document_synchronization":synchronized})
    }

    fn synchronizes(&self) -> bool {
        let sync = &self.capabilities["textDocumentSync"];
        let changes = sync.as_u64().or_else(|| sync["change"].as_u64());
        matches!(changes, Some(1 | 2)) && (sync.is_number() || sync["openClose"] == true)
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        protocol::write_message(
            &mut self.input,
            &json!({"jsonrpc":"2.0","method":method,"params":params}),
        )
        .await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            // LSP ContentModified asks the client to retry after the server's
            // graph changes. Restarting here would repeatedly reset cold loading.
            for attempt in 0..8 {
                match self.request_inner(method, params.clone()).await {
                    Err(error)
                        if error.downcast_ref::<Pending>().is_some_and(|pending| {
                            matches!(pending, Pending::ContentModified | Pending::ServerCancelled)
                        }) && attempt < 7 =>
                    {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    result => return result,
                }
            }
            unreachable!()
        })
        .await
        .context("language server exceeded its 20 second request limit")?
    }

    fn project_state(&self) -> &'static str {
        match self.status.as_ref() {
            Some(status) if status["health"] == "error" => "unavailable",
            Some(status) if status["quiescent"] == false => "pending",
            Some(status) if status["health"] == "warning" => "limited",
            _ => "ready",
        }
    }

    async fn request_inner(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .context("language request identity exhausted")?;
        protocol::write_message(
            &mut self.input,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )
        .await?;
        for _ in 0..MAX_MESSAGES {
            let message = protocol::read_message(&mut self.output).await?;
            ensure!(
                message["jsonrpc"] == "2.0",
                "invalid language server JSON-RPC version"
            );
            if message.get("method").is_some() {
                self.handle(message).await?;
                continue;
            }
            ensure!(
                message["id"] == id,
                "unexpected language server response identity"
            );
            if let Some(error) = message.get("error") {
                if error["code"] == -32801 {
                    return Err(Pending::ContentModified.into());
                }
                if error["code"] == -32802 && error["data"]["retriggerRequest"] == true {
                    return Err(Pending::ServerCancelled.into());
                }
                bail!("language server rejected {method}: {error}");
            }
            return message
                .get("result")
                .cloned()
                .context("language server response has no result");
        }
        bail!("language server exceeded the per-request message bound")
    }

    async fn handle(&mut self, message: Value) -> Result<()> {
        let method = message["method"]
            .as_str()
            .context("invalid language server method")?;
        if let Some(id) = message.get("id") {
            let reply = match method {
                "workspace/configuration" => {
                    let items = message["params"]["items"]
                        .as_array()
                        .context("invalid configuration request")?;
                    ensure!(items.len() <= 128, "too many language configuration items");
                    json!({"jsonrpc":"2.0","id":id,"result":vec![Value::Null;items.len()]})
                }
                "workspace/applyEdit" => {
                    json!({"jsonrpc":"2.0","id":id,"result":{"applied":false,"failureReason":"server-initiated edits are not authorized"}})
                }
                _ => {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"server-initiated operation is not supported"}})
                }
            };
            return protocol::write_message(&mut self.input, &reply).await;
        }
        if method == "experimental/serverStatus" {
            let params = &message["params"];
            ensure!(
                matches!(params["health"].as_str(), Some("ok" | "warning" | "error"))
                    && params["quiescent"].is_boolean(),
                "invalid language project status"
            );
            ensure!(
                params["message"].is_null()
                    || params["message"]
                        .as_str()
                        .is_some_and(|text| text.len() <= 16 * 1024),
                "language project status message exceeds 16 KiB"
            );
            self.status = Some(
                json!({"health":params["health"],"quiescent":params["quiescent"],"message":params["message"]}),
            );
        }
        if method == "textDocument/publishDiagnostics" {
            let params = &message["params"];
            if let Some(document) = params["uri"]
                .as_str()
                .and_then(|uri| self.documents.get_mut(uri))
            {
                let diagnostics = params["diagnostics"]
                    .as_array()
                    .context("invalid diagnostic list")?;
                let version = match params.get("version") {
                    None | Some(Value::Null) => None,
                    Some(version) => Some(i32::try_from(
                        version.as_i64().context("invalid diagnostic version")?,
                    )?),
                };
                if version.is_some_and(|v| v != document.version) {
                    return Ok(());
                }
                document.diagnostics = Some(Value::Array(diagnostics.clone()));
                document.diagnostic_version = version;
                if version.is_some() {
                    document.versioned_diagnostics = document.diagnostics.clone();
                }
            }
        }
        Ok(())
    }

    async fn synchronize(
        &mut self,
        uri: &str,
        language_id: &str,
        text: &str,
        saved: bool,
    ) -> Result<()> {
        ensure!(
            self.synchronizes(),
            "language server does not support document open/change synchronization"
        );
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        if self.documents.get(uri).is_some_and(|d| d.digest == digest) {
            if saved {
                self.saved(uri, text).await?;
            }
            return Ok(());
        }
        // Do not reuse a version when a bounded-cache eviction closes and later
        // reopens the same URI: delayed notifications may still be in the pipe.
        let version = self.next_version;
        self.next_version = version
            .checked_add(1)
            .context("document revision exhausted; reopen session")?;
        if self.documents.contains_key(uri) {
            self.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]})).await?;
        } else {
            if self.documents.len() >= MAX_DOCUMENTS {
                let old = self
                    .documents
                    .keys()
                    .next()
                    .cloned()
                    .context("missing oldest document")?;
                self.notify("textDocument/didClose", json!({"textDocument":{"uri":old}}))
                    .await?;
                self.documents.remove(&old);
            }
            self.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":language_id,"version":version,"text":text}})).await?;
        }
        self.documents.insert(
            uri.into(),
            Document {
                version,
                digest,
                diagnostics: None,
                diagnostic_version: None,
                versioned_diagnostics: None,
            },
        );
        if saved {
            self.saved(uri, text).await?;
        }
        Ok(())
    }

    async fn saved(&mut self, uri: &str, text: &str) -> Result<()> {
        let option = &self.capabilities["textDocumentSync"]["save"];
        if option == true || option.is_object() {
            let mut params = json!({"textDocument":{"uri":uri}});
            if option["includeText"] == true {
                params["text"] = json!(text);
            }
            self.notify("textDocument/didSave", params).await?;
        }
        Ok(())
    }

    async fn diagnostics(&mut self, uri: &str) -> Result<Value> {
        if self.capable("diagnosticProvider") {
            let response = self
                .request(
                    "textDocument/diagnostic",
                    json!({"textDocument":{"uri":uri}}),
                )
                .await?;
            ensure!(
                response["kind"] == "full",
                "language server returned diagnostics without a full current report"
            );
            let items = response["items"]
                .as_array()
                .context("language server omitted diagnostic items")?;
            return self
                .documents
                .get(uri)
                .context("missing diagnostic document")?
                .diagnostic_report(Some(items));
        }
        // A diagnostic set may change for the same source revision while the
        // server indexes dependencies. Drain newer notifications before using a
        // cached set. fill_buf does not consume partial frame bytes, so its
        // timeout leaves the protocol usable; a full frame read is governed by
        // the outer request deadline, whose cancellation destroys this server.
        let deadline = tokio::time::Instant::now() + DIAGNOSTIC_WAIT;
        for index in 0..MAX_MESSAGES {
            let has_diagnostics = self
                .documents
                .get(uri)
                .context("missing diagnostic document")?
                .diagnostics
                .is_some();
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                if !has_diagnostics {
                    return Err(Pending::Diagnostics.into());
                }
                break;
            }
            let wait = if has_diagnostics {
                remaining.min(Duration::from_millis(50))
            } else {
                remaining
            };
            match tokio::time::timeout(wait, self.output.fill_buf()).await {
                Err(_) => {
                    if !has_diagnostics {
                        return Err(Pending::Diagnostics.into());
                    }
                    break;
                }
                Ok(ready) => ensure!(
                    !ready?.is_empty(),
                    "language server closed its diagnostic stream"
                ),
            }
            let message = protocol::read_message(&mut self.output).await?;
            ensure!(
                message["jsonrpc"] == "2.0" && message.get("method").is_some(),
                "unexpected language message while waiting for diagnostics"
            );
            self.handle(message).await?;
            ensure!(
                index + 1 < MAX_MESSAGES,
                "language server exceeded diagnostic message bound"
            );
        }
        let document = self
            .documents
            .get(uri)
            .context("missing diagnostic document")?;
        document.diagnostic_report(None)
    }
}

#[cfg(test)]
mod admission_observer_tests {
    use super::*;

    #[tokio::test]
    async fn a_lone_event_flushes_without_another_event_or_query() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("source.rs");
        std::fs::write(&path, "old").unwrap();
        let root = Arc::new(File::open(workspace.path()).unwrap());
        let access = Arc::new(DeveloperAccess::new(workspace.path(), &[]).unwrap());
        let mut manager = Manager::new(
            LanguageServers::default(),
            workspace.path().to_owned(),
            root,
            access,
        );
        manager.start_observer().unwrap();
        {
            let mut state = manager.state.lock().await;
            state
                .refresh(
                    &manager.config,
                    &manager.workspace,
                    &manager.root,
                    &manager.access,
                )
                .await
                .unwrap();
        }
        // Let the observer install its watch before producing the final event.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let before = manager.state.lock().await.view.as_ref().unwrap().manifest[&path];
        std::fs::write(&path, "new").unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if manager.state.lock().await.view.as_ref().unwrap().manifest[&path] != before {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("final observer event must publish without an explicit query");
        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn observer_recovers_after_a_failed_reconciliation_without_a_query() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("source.rs"), "source").unwrap();
        let root = Arc::new(File::open(workspace.path()).unwrap());
        let access = Arc::new(DeveloperAccess::new(workspace.path(), &[]).unwrap());
        let mut manager = Manager::new(
            LanguageServers::default(),
            workspace.path().to_owned(),
            root,
            access,
        );
        manager.start_observer().unwrap();
        manager
            .state
            .lock()
            .await
            .refresh(
                &manager.config,
                &manager.workspace,
                &manager.root,
                &manager.access,
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(workspace.path().join(".gitignore"), [0xff]).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while manager.state.lock().await.view.is_some() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("invalid ignore input must retire the old view");
        assert!(manager.state.lock().await.refresh_error.is_some());
        std::fs::write(workspace.path().join(".gitignore"), "").unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while manager.state.lock().await.view.is_none() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("corrected input must recover through the observer alone");
        assert!(manager.state.lock().await.refresh_error.is_none());
        manager.stop().await.unwrap();
    }

    #[test]
    fn revocation_stops_peers_before_a_blocked_scan_but_private_additions_do_not() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            for change in ["policy", "deletion", "private"] {
                let workspace = tempfile::tempdir().unwrap();
                std::fs::write(workspace.path().join("public.rs"), "public").unwrap();
                let root = Arc::new(File::open(workspace.path()).unwrap());
                let access = Arc::new(DeveloperAccess::new(workspace.path(), &[]).unwrap());
                let mut state = State::default();
                state
                    .refresh(
                        &LanguageServers::default(),
                        workspace.path(),
                        &root,
                        &access,
                    )
                    .await
                    .unwrap();
                let mut child = tokio::process::Command::new("/usr/bin/sleep")
                    .arg("30")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .unwrap();
                let pid = child.id().unwrap();
                state.servers.insert(
                    Language::Rust,
                    Server {
                        input: child.stdin.take().unwrap(),
                        output: BufReader::new(child.stdout.take().unwrap()),
                        _child: child,
                        capabilities: Value::Null,
                        next_id: 1,
                        documents: BTreeMap::new(),
                        next_version: 1,
                        status: None,
                    },
                );
                let changed = match change {
                    "policy" => {
                        let path = workspace.path().join(".gitignore");
                        std::fs::write(&path, "public.rs\n").unwrap();
                        path
                    }
                    "deletion" => {
                        let path = workspace.path().join("public.rs");
                        std::fs::remove_file(&path).unwrap();
                        path
                    }
                    _ => {
                        let path = workspace.path().join(".demoncoder");
                        std::fs::create_dir(&path).unwrap();
                        path
                    }
                };
                state.changes.lock().unwrap().paths.insert(changed);
                // Occupy the only blocking worker. Revocation must stop the peer
                // before either metadata validation or candidate copying can run.
                let (release, blocked) = std::sync::mpsc::channel();
                let (started, ready) = tokio::sync::oneshot::channel();
                let blocker = tokio::task::spawn_blocking(move || {
                    let _ = started.send(());
                    let _ = blocked.recv();
                });
                ready.await.unwrap();
                let state = Arc::new(Mutex::new(state));
                let owner = state.clone();
                let path = workspace.path().to_owned();
                let refresh = tokio::spawn(async move {
                    owner
                        .lock()
                        .await
                        .refresh(&LanguageServers::default(), &path, &root, &access)
                        .await
                });
                let stopped = tokio::time::timeout(Duration::from_millis(300), async {
                    while Path::new(&format!("/proc/{pid}")).exists() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .is_ok();
                // Always release before asserting, including a failing case, so
                // the regression itself cannot strand the runtime's worker.
                release.send(()).unwrap();
                let result = refresh.await.unwrap();
                blocker.await.unwrap();
                assert!(result.is_ok(), "{change}: {result:?}");
                assert_eq!(stopped, change != "private", "{change}");
                assert_eq!(
                    state.lock().await.servers.is_empty(),
                    change != "private",
                    "{change}"
                );
                state.lock().await.stop().await.unwrap();
            }
        });
    }

    #[tokio::test]
    async fn trailing_flush_coalesces_and_busy_events_have_a_maximum_delay() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let producer = tokio::spawn(async move {
            loop {
                let _ = tx.try_send(());
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        let start = tokio::time::Instant::now();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), debounce(&mut rx))
                .await
                .unwrap()
        );
        assert!(
            start.elapsed() >= Duration::from_millis(900),
            "busy batch was not coalesced"
        );
        producer.abort();
        let (_tx, mut rx) = tokio::sync::mpsc::channel(1);
        let start = tokio::time::Instant::now();
        assert!(debounce(&mut rx).await);
        assert!(start.elapsed() >= Duration::from_millis(190));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
