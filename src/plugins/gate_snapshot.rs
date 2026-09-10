//! Immutable filesystem input to a later gate runner. No gate execution or
//! admission happens here. Recapture is a precheck, not an atomic transaction
//! with outside writers. Callers must repeat it at their mutation boundary.
//!
//! All APIs are synchronous and bounded at scanner checkpoints; callers running
//! them on a blocking worker must set the cancellation flag when their owner is
//! dropped. A blocked filesystem syscall cannot be interrupted by that flag.
//! Materializers must preserve the retained ownership and ACLs or hold; existing
//! byte/chmod materialization alone does not establish that guarantee.
use crate::workflow::workspace;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub use workspace::{AccessMetadata, AclMetadata, GateReadSet, Kind as EntryKind};

pub const FORMAT_VERSION: u8 = 1;

/// A descriptor pins the admitted root for both capture and freshness checks.
/// Opening another directory at the same pathname never replaces this identity.
pub struct GateWorkspace {
    root: PathBuf,
    pinned: File,
    credentials: Vec<PathBuf>,
    protected: Result<Vec<PathBuf>, CaptureError>,
}
impl std::fmt::Debug for GateWorkspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GateWorkspace(<pinned>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    Cancelled,
    AccessMetadata(String),
    /// A missing, changing, oversized, protected, or unrepresentable read set
    /// holds inspection. Source paths and captured content are never diagnostics.
    Unavailable,
}
impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Self::Cancelled => "gate snapshot capture cancelled", Self::AccessMetadata(message) => message, Self::Unavailable => "required gate snapshot is unavailable; inspect workspace bounds, access and read-set declarations" })
    }
}
impl std::error::Error for CaptureError {}

impl GateWorkspace {
    pub fn open(root: &Path) -> Result<Self, CaptureError> {
        Self::open_with_credentials(root, &[])
    }
    pub(crate) fn open_with_credentials(
        root: &Path,
        credentials: &[PathBuf],
    ) -> Result<Self, CaptureError> {
        let root = std::path::absolute(root).map_err(|_| CaptureError::Unavailable)?;
        let pinned = workspace::open_gate_root(&root).map_err(|_| CaptureError::Unavailable)?;
        if credentials.len() > 128 {
            return Err(CaptureError::Unavailable);
        }
        let configured = credentials
            .iter()
            .map(|path| {
                if path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(CaptureError::Unavailable);
                }
                std::path::absolute(path).map_err(|_| CaptureError::Unavailable)
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Preserve explicit-credential constructor validation. Default host roots
        // have historically held capture, not construction of ordinary executors.
        protected_paths(&root, &configured)?;
        // Expand BOTH launch-CWD and workspace-relative meanings before making
        // paths absolute. Freeze once; invocation must not reinterpret host policy.
        let mut credentials = crate::export_policy::private_roots(&root, credentials, true)
            .into_iter()
            .map(std::path::absolute)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CaptureError::Unavailable)?;
        credentials.sort();
        credentials.dedup();
        let protected = protected_paths(&root, &credentials);
        Ok(Self {
            root,
            pinned,
            credentials,
            protected,
        })
    }
    pub(crate) fn frozen_credentials(&self) -> Vec<PathBuf> {
        let mut roots = self.credentials.clone();
        if let Ok(protected) = &self.protected {
            // Retain the physical targets captured with the policy as well as its
            // lexical aliases. A later alias remap cannot expose an old target
            // when a live parent grant overlays the retained snapshot.
            roots.extend(protected.iter().map(|path| self.root.join(path)));
        }
        roots.sort();
        roots.dedup();
        roots
    }
    pub fn capture(
        &self,
        read_set: &GateReadSet,
        cancelled: &AtomicBool,
    ) -> Result<GateSnapshot, CaptureError> {
        let protected = self.protected.as_ref().map_err(Clone::clone)?;
        if &protected_paths(&self.root, &self.credentials)? != protected {
            return Err(CaptureError::Unavailable);
        }
        let started = Instant::now();
        let checkpoint = || -> anyhow::Result<()> {
            anyhow::ensure!(
                !cancelled.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(10),
                "gate capture cancelled or timed out"
            );
            Ok(())
        };
        let (snapshot, mut raw) =
            workspace::capture_gate(&self.root, &self.pinned, read_set, protected, cancelled)
                .map_err(|error| {
                    if cancelled.load(Ordering::Relaxed) {
                        CaptureError::Cancelled
                    } else if let Some(error) =
                        error.downcast_ref::<workspace::AccessCaptureError>()
                    {
                        CaptureError::AccessMetadata(error.to_string())
                    } else {
                        CaptureError::Unavailable
                    }
                })?;
        let mut entries = BTreeMap::new();
        let matches = read_set
            .matches(&snapshot.entries, &checkpoint)
            .map_err(|_| {
                if cancelled.load(Ordering::Relaxed) {
                    CaptureError::Cancelled
                } else {
                    CaptureError::Unavailable
                }
            })?;
        let root_identity = snapshot.root_identity();
        let revision = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(FORMAT_VERSION, read_set, &snapshot.digest, &matches))
                    .map_err(|_| CaptureError::Unavailable)?
            )
        );
        for (path, metadata) in snapshot.entries {
            let bytes = raw.remove(&path).ok_or(CaptureError::Unavailable)?;
            let name = path.strip_prefix("./").unwrap_or(&path).to_owned();
            entries.insert(
                name,
                CapturedEntry {
                    kind: metadata.kind,
                    mode: metadata.mode,
                    access: metadata.access.ok_or(CaptureError::Unavailable)?,
                    bytes,
                },
            );
        }
        let memberships = snapshot
            .memberships
            .into_iter()
            .map(|(name, members)| (name.strip_prefix("./").unwrap_or(&name).to_owned(), members))
            .collect();
        checkpoint().map_err(|_| {
            if cancelled.load(Ordering::Relaxed) {
                CaptureError::Cancelled
            } else {
                CaptureError::Unavailable
            }
        })?;
        if &protected_paths(&self.root, &self.credentials)? != protected {
            return Err(CaptureError::Unavailable);
        }
        Ok(GateSnapshot {
            revision,
            root_identity,
            read_set: read_set.clone(),
            entries,
            memberships,
            matches,
        })
    }
    /// Failure to recapture is an error/hold, never freshness. A false return is
    /// a successfully observed different revision. Neither result authorizes work.
    pub fn is_current(
        &self,
        snapshot: &GateSnapshot,
        cancelled: &AtomicBool,
    ) -> Result<bool, CaptureError> {
        let current = self.capture(&snapshot.read_set, cancelled)?;
        Ok(
            snapshot.root_identity == current.root_identity
                && snapshot.revision == current.revision,
        )
    }
}

// Resolve even an absent credential through its nearest existing ancestor. Keep
// its lexical exclusion as well, and repeat this mapping around every capture.
pub(crate) fn protected_paths(
    root: &Path,
    credentials: &[PathBuf],
) -> Result<Vec<PathBuf>, CaptureError> {
    if credentials.len() > 512 {
        return Err(CaptureError::Unavailable);
    }
    let mut protected = Vec::new();
    for credential in credentials {
        let mut ancestor = credential.as_path();
        let mut missing = Vec::new();
        let mut physical = loop {
            match ancestor.canonicalize() {
                Ok(path) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // A dangling symlink is not an absent lexical component:
                    // discarding it would lose its missing credential target.
                    // Also hold a path that changed between these observations.
                    if !matches!(ancestor.symlink_metadata(), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
                    {
                        return Err(CaptureError::Unavailable);
                    }
                    missing.push(ancestor.file_name().ok_or(CaptureError::Unavailable)?);
                    ancestor = ancestor.parent().ok_or(CaptureError::Unavailable)?;
                }
                Err(_) => return Err(CaptureError::Unavailable),
            }
        };
        for component in missing.into_iter().rev() {
            physical.push(component);
        }
        for path in [credential, &physical] {
            if root.starts_with(path) {
                return Err(CaptureError::Unavailable);
            }
            if let Ok(relative) = path.strip_prefix(root) {
                protected.push(Path::new(".").join(relative));
            }
        }
    }
    protected.sort();
    protected.dedup();
    Ok(protected)
}

/// Fresh captures only: no deserializer can turn historical metadata or supplied
/// bytes into current evidence. FORMAT_VERSION identifies the revision encoding;
/// durable admission records may store the revision, then must recapture on use.
pub struct GateSnapshot {
    revision: String,
    root_identity: (u64, u64),
    read_set: GateReadSet,
    entries: BTreeMap<String, CapturedEntry>,
    memberships: BTreeMap<String, Vec<String>>,
    matches: BTreeMap<String, Vec<String>>,
}
impl std::fmt::Debug for GateSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GateSnapshot")
            .field("revision", &self.revision)
            .field("entries", &self.entries.len())
            .finish()
    }
}
impl GateSnapshot {
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn root_identity(&self) -> (u64, u64) {
        self.root_identity
    }
    /// Canonical relative names; the root's metadata is available under `.`.
    pub fn entry(&self, path: &str) -> Option<&CapturedEntry> {
        self.entries.get(path)
    }
    /// Deliberate evidence access; neither Debug nor diagnostics emits these names.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &CapturedEntry)> {
        self.entries.iter().map(|(p, e)| (p.as_str(), e))
    }
    pub fn absent_paths(&self) -> &[String] {
        self.read_set.absent_paths()
    }
    pub fn glob_matches(&self) -> &BTreeMap<String, Vec<String>> {
        &self.matches
    }
    /// Only selected directories appear here. Supporting ancestors retain metadata
    /// but do not claim complete directory inspection. Protected names are omitted
    /// for the legacy full-workspace default; explicit required private reads hold.
    pub fn memberships(&self) -> &BTreeMap<String, Vec<String>> {
        &self.memberships
    }
}

pub struct CapturedEntry {
    kind: EntryKind,
    mode: u32,
    access: AccessMetadata,
    bytes: Vec<u8>,
}
impl std::fmt::Debug for CapturedEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedEntry")
            .field("kind", &self.kind)
            .field("mode", &self.mode)
            .field("access", &self.access)
            .field("byte_count", &self.bytes.len())
            .finish()
    }
}
impl CapturedEntry {
    pub fn kind(&self) -> &EntryKind {
        &self.kind
    }
    pub fn mode(&self) -> u32 {
        self.mode
    }
    pub fn access(&self) -> &AccessMetadata {
        &self.access
    }
    /// Original file bytes, literal symlink target bytes, or empty for a directory.
    /// This borrow never consults the live workspace.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
