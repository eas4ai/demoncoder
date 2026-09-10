//! Private authenticated channel between the managed Codex command and its owner.
use super::bridge::{Callbacks, Lifecycle};
use crate::events::EventSink;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

const MAX_FRAME: usize = 1024 * 1024;
const PROTOCOL: &str = "demoncoder-compaction-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Address {
    socket: PathBuf,
    token: String,
}

pub(crate) struct Owner {
    _root: tempfile::TempDir,
    _parent: std::fs::File,
    protected_root: PathBuf,
    listener: UnixListener,
    token: String,
    callbacks: Callbacks,
    requirement: String,
}

fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

impl Owner {
    pub(crate) fn new(lifecycle: &Arc<Lifecycle>, executable: &Path) -> Result<Self> {
        // The shared developer tool boundary protects this parent in every
        // session, including sessions opened before this relay exists.
        let parent_path = PathBuf::from(std::env::var_os("HOME").context("relay requires HOME")?)
            .canonicalize()?
            .join(".demoncoder");
        crate::workflow::store::private_directory(&parent_path)?;
        let parent: std::fs::File = rustix::fs::openat2(
            rustix::fs::CWD,
            &parent_path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
            rustix::fs::ResolveFlags::NO_SYMLINKS,
        )?
        .into();
        let pinned_parent = PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd()));
        let root = tempfile::Builder::new()
            .prefix("relay-")
            .tempdir_in(pinned_parent)?;
        let protected_root =
            parent_path.join(root.path().file_name().context("relay root name missing")?);
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
        let mut entropy = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut entropy)?;
        let token = entropy
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let address = Address {
            socket: protected_root.join("relay.sock"),
            token: token.clone(),
        };
        let listener = UnixListener::bind(root.path().join("relay.sock"))?;
        let address_path = root.path().join("address.json");
        std::fs::write(&address_path, serde_json::to_vec(&address)?)?;
        std::fs::set_permissions(&address_path, std::fs::Permissions::from_mode(0o600))?;
        let command = format!(
            "{} --codex-compaction-relay {}",
            shell_word(
                executable
                    .to_str()
                    .context("relay executable path is not UTF-8")?
            ),
            shell_word(
                protected_root
                    .join("address.json")
                    .to_str()
                    .context("relay address path is not UTF-8")?
            )
        );
        let source = root.path().join("hooks.json");
        let mut hooks = serde_json::Map::new();
        for event in ["PreCompact", "PostCompact"] {
            hooks.insert(
                event.into(),
                json!([{"hooks":[{"type":"command","command":command,"timeout":65}]}]),
            );
        }
        let bytes = serde_json::to_vec(&json!({"hooks":hooks}))?;
        std::fs::write(&source, &bytes)?;
        let requirement = serde_json::to_string(
            &json!({"protocol":PROTOCOL,"source_path":protected_root.join("hooks.json"),"source_sha256":format!("{:x}", Sha256::digest(&bytes)),"command":command}),
        )?;
        Ok(Self {
            _root: root,
            _parent: parent,
            protected_root,
            listener,
            token,
            callbacks: lifecycle.callbacks()?,
            requirement,
        })
    }

    pub(crate) fn protected_root(&self) -> &Path {
        &self.protected_root
    }

    pub(crate) fn requirement(&self) -> &str {
        &self.requirement
    }

    /// No success is written before host admission. EOF/error remains denial in
    /// the patched backend, even when this owner or a lifecycle worker dies.
    pub(crate) async fn accept(&self) -> Result<UnixStream> {
        Ok(self.listener.accept().await?.0)
    }

    pub(crate) async fn handle(
        &mut self,
        mut stream: UnixStream,
        session: &str,
        turn: &str,
        events: &EventSink,
    ) -> Result<()> {
        let peer = stream.peer_cred()?;
        ensure!(
            peer.uid() == rustix::process::getuid().as_raw(),
            "lifecycle relay owner mismatch"
        );
        let request = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stream))
            .await
            .context("lifecycle relay input timed out")??;
        ensure!(
            request["token"].as_str() == Some(self.token.as_str()),
            "lifecycle relay authentication failed"
        );
        let input = request["input"].clone();
        let decision = self
            .callbacks
            .handle_codex(input.clone(), session, turn, events)
            .await;
        let response = match &decision {
            Ok(response) => response.clone(),
            Err(_) => {
                json!({"continue":false,"stopReason":"Host lifecycle admission failed; work remains held",
                "demonCoderCompaction":{"protocol":PROTOCOL,"challenge":input["demonCoderCompaction"]["challenge"],
                    "session_id":session,"turn_id":turn,"hook_event_name":input["hook_event_name"]}})
            }
        };
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(5), stream.write_all(&bytes))
            .await
            .context("lifecycle relay acknowledgment timed out")??;
        decision.map(|_| ())
    }
}

async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> Result<Value> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_FRAME as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(
        bytes.len() <= MAX_FRAME,
        "lifecycle relay frame exceeds limit"
    );
    serde_json::from_slice(&bytes).context("invalid lifecycle relay frame")
}

/// Internal executable entry point. The command is only a transport; it cannot
/// grant permission itself and never invents an acknowledgment on failure.
pub async fn run(address_path: &Path) -> Result<()> {
    let mut address_bytes = Vec::new();
    std::fs::File::open(address_path)
        .context("open private lifecycle relay address")?
        .take(4097)
        .read_to_end(&mut address_bytes)?;
    ensure!(
        address_bytes.len() <= 4096,
        "lifecycle relay address exceeds limit"
    );
    let address: Address = serde_json::from_slice(&address_bytes)?;
    let exchange = async {
        let input = read_frame(&mut tokio::io::stdin()).await?;
        let mut stream = UnixStream::connect(&address.socket).await?;
        ensure!(
            stream.peer_cred()?.uid() == rustix::process::getuid().as_raw(),
            "lifecycle relay owner mismatch"
        );
        let request = serde_json::to_vec(&json!({"token":address.token,"input":input}))?;
        ensure!(
            request.len() <= MAX_FRAME,
            "lifecycle relay input exceeds limit"
        );
        stream.write_all(&request).await?;
        stream.shutdown().await?;
        let response = read_frame(&mut stream).await?;
        // The host constructs typed output. The backend independently checks
        // its challenge/session/turn/event before releasing its boundary.
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        tokio::io::stdout().write_all(&bytes).await?;
        Ok::<(), anyhow::Error>(())
    };
    tokio::time::timeout(Duration::from_secs(64), exchange)
        .await
        .context("lifecycle relay timed out")?
}

#[cfg(test)]
mod tests {
    use super::super::bridge::{Decision, Handler, Invocation};
    use super::*;
    use async_trait::async_trait;
    struct Failure;
    #[async_trait]
    impl Handler for Failure {
        async fn handle(&self, _: &Invocation, _: &EventSink) -> Result<Decision> {
            anyhow::bail!("controlled worker failure")
        }
    }

    #[tokio::test]
    async fn live_relay_returns_correlated_denial_on_worker_failure() -> Result<()> {
        let lifecycle = Arc::new(Lifecycle::new(Arc::new(Failure), Duration::from_secs(1))?);
        let mut owner = Owner::new(&lifecycle, Path::new("/usr/bin/true"))?;
        let address: Address =
            serde_json::from_slice(&std::fs::read(owner._root.path().join("address.json"))?)?;
        let client = tokio::spawn(async move {
            let mut stream = UnixStream::connect(address.socket).await?;
            let input = json!({"session_id":"session", "turn_id":"turn", "hook_event_name":"PreCompact", "trigger":"auto", "demonCoderCompaction":{"protocol":PROTOCOL,"challenge":"fresh"}});
            stream
                .write_all(&serde_json::to_vec(
                    &json!({"token":address.token,"input":input}),
                )?)
                .await?;
            stream.shutdown().await?;
            read_frame(&mut stream).await
        });
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let events = EventSink::new("relay-error".into(), tx, None)?;
        let stream = owner.accept().await?;
        let error = owner
            .handle(stream, "session", "turn", &events)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("controlled worker failure"));
        let response = client.await??;
        assert_eq!(response["continue"], false);
        assert_eq!(
            response["demonCoderCompaction"],
            json!({"protocol":PROTOCOL,"challenge":"fresh","hook_event_name":"PreCompact","session_id":"session","turn_id":"turn"})
        );
        Ok(())
    }

    #[tokio::test]
    async fn relay_frames_reject_oversize_and_ambiguous_trailing_data() {
        let oversized = vec![b' '; MAX_FRAME + 1];
        assert!(read_frame(&mut oversized.as_slice()).await.is_err());
        assert!(read_frame(&mut b"{}{}".as_slice()).await.is_err());
        assert!(read_frame(&mut b"{invalid".as_slice()).await.is_err());
    }
}
