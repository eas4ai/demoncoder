//! Bounded, read-only Linux workspace capture. No Git commands, filters or hooks
//! run. Private source and the root `.git` entry are excluded; Git administrative changes are
//! therefore outside the acceptance identity. Ignored and untracked files count.
//!
//! Two matching scans detect ordinary concurrent edits, not an atomic filesystem
//! transaction. A hostile process can still edit after capture; callers must
//! recapture immediately before using evidence. Deadlines are cooperative: a
//! stalled filesystem syscall cannot be interrupted by this synchronous module.

use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{Dir, Mode, OFlags, ResolveFlags, openat2, readlinkat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod scope;
pub use scope::CaptureScope;

/// Only the worktree owner can admit child source inside the session container.
/// Other credential roots and private entry names remain protected in both modes.
#[derive(Clone, Copy)]
pub(crate) enum CaptureRoot {
    Workspace,
    OwnedWorktree,
}

const MAX_ENTRIES: usize = 20_000;
const MAX_DEPTH: usize = 64;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EVIDENCE_BYTES: usize = 1024 * 1024;
const MAX_DURATION: Duration = Duration::from_secs(10);
const RESOLVE: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_XDEV);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub digest: String,
    /// Zero identifies historical snapshots captured before private-source exclusions.
    #[serde(default)]
    export_policy: u8,
    #[serde(default, skip_serializing_if = "CaptureScope::is_empty")]
    pub scope: CaptureScope,
    root_device: u64,
    root_inode: u64,
    pub(crate) entries: BTreeMap<String, Entry>,
}

impl Snapshot {
    pub(crate) fn root_identity(&self) -> (u64, u64) {
        (self.root_device, self.root_inode)
    }

    pub(crate) fn same_root(&self, other: &Self) -> bool {
        self.root_device == other.root_device && self.root_inode == other.root_inode
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) kind: Kind,
    pub(crate) mode: u32,
    bytes: u64,
    hash: String,
    text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Kind {
    Directory,
    File,
    Symlink,
}

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    mode: u32,
    length: u64,
    links: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn stamp(file: &File) -> Result<Stamp> {
    let m = file.metadata()?;
    Ok(Stamp {
        device: m.dev(),
        inode: m.ino(),
        mode: m.mode(),
        length: m.len(),
        links: m.nlink(),
        modified: (m.mtime(), m.mtime_nsec()),
        changed: (m.ctime(), m.ctime_nsec()),
    })
}

struct Scan<'a> {
    root: &'a File,
    started: Instant,
    entries: BTreeMap<String, Entry>,
    stamps: BTreeMap<String, Stamp>,
    raw: Option<BTreeMap<String, Vec<u8>>>,
    cancelled: Option<&'a AtomicBool>,
    scope: &'a CaptureScope,
    bytes: u64,
}

impl Scan<'_> {
    fn checkpoint(&self) -> Result<()> {
        ensure!(
            !self
                .cancelled
                .is_some_and(|flag| flag.load(Ordering::Relaxed)),
            "workspace capture cancelled"
        );
        ensure!(
            self.started.elapsed() < MAX_DURATION,
            "workspace capture exceeded 10 seconds; select a smaller local workspace"
        );
        Ok(())
    }

    fn open(&self, path: &Path, flags: OFlags) -> Result<File> {
        Ok(openat2(self.root, path, flags | OFlags::CLOEXEC | OFlags::NOFOLLOW, Mode::empty(), RESOLVE)
            .with_context(|| format!("cannot securely capture {}; symlink traversal and mounted subtrees are not supported", path.display()))?.into())
    }

    fn walk(&mut self, path: &Path, depth: usize) -> Result<()> {
        self.checkpoint()?;
        ensure!(
            depth <= MAX_DEPTH,
            "workspace exceeds capture depth limit of {MAX_DEPTH}; select a smaller workspace"
        );
        ensure!(
            self.entries.len() < MAX_ENTRIES,
            "workspace exceeds {MAX_ENTRIES} captured entries; select a smaller workspace"
        );
        let name = path
            .to_str()
            .context("workspace contains a non-UTF-8 path; rename it before verification")?
            .to_owned();
        let handle = self.open(path, OFlags::PATH)?;
        let before = stamp(&handle)?;
        let metadata = handle.metadata()?;
        let kind;
        let content;
        if metadata.is_dir() {
            kind = Kind::Directory;
            content = Vec::new();
            let dir = self.open(path, OFlags::RDONLY | OFlags::DIRECTORY)?;
            ensure!(
                stamp(&dir)? == before,
                "directory changed during capture: {name}; retry when edits stop"
            );
            // Reserve this entry before recursion so the global entry limit
            // also bounds wide trees and ancestor directories.
            self.entries
                .insert(name.clone(), entry(kind.clone(), metadata.mode(), &content));
            for item in Dir::read_from(&dir)? {
                self.checkpoint()?;
                let item = item?;
                let bytes = item.file_name().to_bytes();
                if bytes == b"." || bytes == b".." || (depth == 0 && bytes == b".git") {
                    continue;
                }
                let child = std::str::from_utf8(bytes).context(
                    "workspace contains a non-UTF-8 path; rename it before verification",
                )?;
                if crate::export_policy::private_path(&path.join(child))
                    || self.scope.excludes(&path.join(child))
                {
                    continue;
                }
                self.walk(&path.join(child), depth + 1)?;
            }
        } else if metadata.is_file() {
            kind = Kind::File;
            ensure!(
                metadata.nlink() == 1,
                "cannot capture multiply linked file {name}; copy it into an independent workspace file"
            );
            ensure!(
                metadata.len() <= MAX_FILE_BYTES,
                "file {name} exceeds 8 MiB capture limit; select a smaller workspace"
            );
            // Reopen the pinned, already classified regular inode. Opening the
            // workspace name again would race a replacement device/FIFO. This
            // procfs link belongs to our live descriptor, never to workspace
            // text or a workspace symlink; it cannot redirect to another inode.
            let mut file = File::open(format!("/proc/self/fd/{}", handle.as_raw_fd()))
                .with_context(|| {
                    format!("cannot reopen pinned regular file {name}; Linux procfs is required")
                })?;
            ensure!(
                stamp(&file)? == before,
                "file changed before capture: {name}; retry when edits stop"
            );
            let mut data = Vec::new();
            let mut chunk = vec![0_u8; 64 * 1024];
            loop {
                self.checkpoint()?;
                let count = file
                    .read(&mut chunk)
                    .with_context(|| format!("reading workspace file {name}"))?;
                if count == 0 {
                    break;
                }
                self.bytes += count as u64;
                ensure!(
                    data.len() + count <= MAX_FILE_BYTES as usize,
                    "file {name} grew beyond 8 MiB capture limit"
                );
                ensure!(
                    self.bytes <= MAX_TOTAL_BYTES,
                    "workspace exceeds 64 MiB capture limit; select a smaller workspace"
                );
                data.extend_from_slice(&chunk[..count]);
            }
            ensure!(
                stamp(&file)? == before && data.len() as u64 == before.length,
                "file changed during capture: {name}; retry when edits stop"
            );
            content = data;
        } else if metadata.file_type().is_symlink() {
            kind = Kind::Symlink;
            // Empty-path readlinkat reads the pinned O_PATH symlink itself.
            content = readlinkat(&handle, "", Vec::new())?.into_bytes();
            self.bytes += content.len() as u64;
            ensure!(
                self.bytes <= MAX_TOTAL_BYTES,
                "workspace exceeds 64 MiB capture limit; select a smaller workspace"
            );
        } else {
            bail!(
                "unsupported special file {name}; remove devices, FIFOs and sockets from the selected workspace before verification"
            );
        }
        ensure!(
            stamp(&handle)? == before,
            "workspace entry changed during capture: {name}; retry when edits stop"
        );
        self.entries
            .insert(name.clone(), entry(kind, metadata.mode(), &content));
        if let Some(raw) = &mut self.raw {
            raw.insert(name.clone(), content);
        }
        self.stamps.insert(name, before);
        Ok(())
    }
}

fn entry(kind: Kind, mode: u32, data: &[u8]) -> Entry {
    Entry {
        kind,
        mode,
        bytes: data.len() as u64,
        hash: format!("{:x}", Sha256::digest(data)),
        text: if data.contains(&0) {
            None
        } else {
            String::from_utf8(data.to_vec()).ok()
        },
    }
}

/// Capture public entries below a real directory, including ignored/untracked source.
/// Refuses symlink roots, descendant mounts, hard-linked regular files, special
/// files, non-UTF-8 names and oversized trees rather than silently skipping them.
pub fn capture(root: &Path) -> Result<Snapshot> {
    capture_with_scope(root, &CaptureScope::default())
}

pub fn capture_with_scope(root: &Path, scope: &CaptureScope) -> Result<Snapshot> {
    Ok(capture_inner(root, false, None, scope, CaptureRoot::Workspace)?.0)
}

/// The worktree coordinator can cancel a background read at scanner checkpoints.
/// Raw bytes remain transient and never enlarge serialized evidence records.
#[cfg(test)]
pub(crate) fn capture_cancellable(
    root: &Path,
    retain_raw: bool,
    cancelled: &AtomicBool,
) -> Result<(Snapshot, BTreeMap<String, Vec<u8>>)> {
    capture_scoped_cancellable(
        root,
        retain_raw,
        cancelled,
        &CaptureScope::default(),
        CaptureRoot::Workspace,
    )
}

pub(crate) fn capture_scoped_cancellable(
    root: &Path,
    retain_raw: bool,
    cancelled: &AtomicBool,
    scope: &CaptureScope,
    kind: CaptureRoot,
) -> Result<(Snapshot, BTreeMap<String, Vec<u8>>)> {
    capture_inner(root, retain_raw, Some(cancelled), scope, kind)
}

fn capture_inner(
    root: &Path,
    retain_raw: bool,
    cancelled: Option<&AtomicBool>,
    scope: &CaptureScope,
    kind: CaptureRoot,
) -> Result<(Snapshot, BTreeMap<String, Vec<u8>>)> {
    scope.validate()?;
    ensure!(
        !crate::export_policy::contains_declared_private(
            &root.canonicalize()?,
            &crate::export_policy::private_roots(root, &[], matches!(kind, CaptureRoot::Workspace)),
        )?,
        "workspace overlaps a private credential or runtime root; select a project directory outside those roots"
    );
    let started = Instant::now();
    let open_root = || -> Result<File> {
        Ok(openat2(rustix::fs::CWD, root, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW, Mode::empty(), ResolveFlags::NO_SYMLINKS)
            .context("cannot securely open workspace root; select a real directory without symlink ancestors")?.into())
    };
    let root_fd = open_root()?;
    let root_stamp = stamp(&root_fd)?;
    let scan = |keep_raw: bool| -> Result<Scan<'_>> {
        let mut scan = Scan {
            root: &root_fd,
            started,
            entries: BTreeMap::new(),
            stamps: BTreeMap::new(),
            raw: keep_raw.then(BTreeMap::new),
            cancelled,
            scope,
            bytes: 0,
        };
        scan.walk(Path::new("."), 0)?;
        Ok(scan)
    };
    let first = scan(false)?;
    let second = scan(retain_raw)?;
    ensure!(
        first.entries == second.entries
            && first.stamps == second.stamps
            && stamp(&root_fd)? == root_stamp
            && stamp(&open_root()?)? == root_stamp,
        "workspace changed during capture; stop concurrent edits and retry"
    );
    let mut snapshot = Snapshot {
        digest: String::new(),
        export_policy: crate::export_policy::VERSION,
        scope: scope.clone(),
        root_device: root_stamp.device,
        root_inode: root_stamp.inode,
        entries: second.entries,
    };
    // Structured serialization supplies unambiguous length/delimiter encoding;
    // BTreeMap iteration provides deterministic ordering independent of readdir.
    snapshot.digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&snapshot)?));
    Ok((snapshot, second.raw.unwrap_or_default()))
}

fn append(output: &mut String, text: &str) -> Result<()> {
    ensure!(
        output.len().saturating_add(text.len()) <= MAX_EVIDENCE_BYTES,
        "review evidence exceeds 1 MiB; reduce supporting context. All changed source and omitted identities must still fit"
    );
    output.push_str(text);
    Ok(())
}

fn describe_identity(output: &mut String, side: &str, value: Option<&Entry>) -> Result<()> {
    let Some(value) = value else {
        return append(output, &format!("{side}: absent\n"));
    };
    append(
        output,
        &format!(
            "{side}: {:?}, mode {:o}, {} bytes, sha256 {}\n",
            value.kind, value.mode, value.bytes, value.hash
        ),
    )
}

fn describe(output: &mut String, side: &str, value: Option<&Entry>, changed: bool) -> Result<()> {
    describe_identity(output, side, value)?;
    let Some(value) = value else {
        return Ok(());
    };
    if value.kind != Kind::Directory {
        if let Some(text) = &value.text {
            // JSON escaping keeps arbitrary source text from forging evidence
            // headings; the reviewer receives the complete, reversible contents.
            append(
                output,
                &format!(
                    "{side} complete content (JSON string): {}\n",
                    serde_json::to_string(text)?
                ),
            )?;
        } else {
            ensure!(
                !changed,
                "changed binary/non-UTF-8 content blocks review; a complete textual representation is required"
            );
            append(
                output,
                "Unchanged binary: identity retained; no textual source representation is available.\n",
            )?;
        }
    }
    Ok(())
}

/// Complete old/new changed text plus selected supporting baseline context.
/// Baseline includes pre-existing developer edits and untracked/ignored files;
/// no Git tracked/clean claim is made. Oversized or changed binary evidence fails.
pub fn review_evidence(before: &Snapshot, after: &Snapshot) -> Result<String> {
    ensure!(
        before.export_policy == crate::export_policy::VERSION
            && after.export_policy == crate::export_policy::VERSION,
        "snapshot predates the current private-root export policy; start a new task baseline"
    );
    before.scope.validate()?;
    after.scope.validate()?;
    ensure!(
        before.scope == after.scope,
        "source or review scope changed; start a new task baseline"
    );
    ensure!(
        before.root_device == after.root_device && before.root_inode == after.root_inode,
        "workspace root identity changed; start a new task baseline"
    );
    let mut output = String::new();
    append(
        &mut output,
        &format!(
            "Workspace baseline {}\nCurrent snapshot {}\nScope: all workspace entries including pre-existing changes and untracked/ignored baseline files. Git tracking status is not inferred. Root .git administrative entry is excluded. Symlink contents are literal targets; targets are never read.\n",
            before.digest, after.digest
        ),
    )?;
    append(&mut output, crate::export_policy::DESCRIPTION)?;
    if !after.scope.generated_outputs().is_empty() {
        append(
            &mut output,
            &format!(
                "Generated-output scope (excluded from source capture and review): {}\n",
                serde_json::to_string(after.scope.generated_outputs())?
            ),
        )?;
    }
    let names: std::collections::BTreeSet<_> =
        before.entries.keys().chain(after.entries.keys()).collect();
    if let Some(context) = after.scope.review_context() {
        append(
            &mut output,
            &format!(
                "Review scope: {}. Every changed in-scope entry is complete. Other supporting contents are not reviewed; their identities remain below.\n",
                serde_json::to_string(&after.scope)?
            ),
        )?;
        for path in context {
            ensure!(
                names.iter().any(|name| {
                    let name = Path::new(name);
                    name.strip_prefix(".").unwrap_or(name).starts_with(path)
                        && !crate::export_policy::private_path(name)
                        && !after.scope.excludes(name)
                }),
                "selected review context {path:?} is absent from both snapshots"
            );
        }
    }
    for name in names {
        // Older snapshots may contain private text. Never re-export that content.
        if crate::export_policy::private_path(Path::new(name))
            || after.scope.excludes(Path::new(name))
        {
            continue;
        }
        let old = before.entries.get(name);
        let new = after.entries.get(name);
        let changed = old != new;
        let include_context = after.scope.includes_review_context(Path::new(name));
        append(
            &mut output,
            &format!(
                "\nPath {}: {}\n",
                serde_json::to_string(name)?,
                if changed {
                    "changed since baseline"
                } else if include_context {
                    "pre-existing baseline entry retained as context"
                } else {
                    "unchanged supporting contents omitted; not reviewed"
                }
            ),
        )?;
        if changed {
            describe(
                &mut output,
                "OLD (pre-existing baseline, including any untracked/ignored file)",
                old,
                true,
            )?;
            describe(&mut output, "NEW", new, true)?;
        } else if include_context {
            describe(&mut output, "RETAINED", new, false)?;
        } else {
            describe_identity(&mut output, "OMITTED", new)?;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_identities_must_also_fit_the_review_limit() {
        let root = tempfile::tempdir().unwrap();
        let scope = CaptureScope::default()
            .with_review_context(Some(vec![]))
            .unwrap();
        let mut snapshot = capture_with_scope(root.path(), &scope).unwrap();
        // Isolate the formatter with metadata from a permitted-size source tree.
        for index in 0..4096 {
            snapshot.entries.insert(
                format!("./{index}-{}", "x".repeat(200)),
                entry(Kind::File, 0o600, b""),
            );
        }
        assert!(
            review_evidence(&snapshot, &snapshot)
                .unwrap_err()
                .to_string()
                .contains("1 MiB")
        );
    }

    #[test]
    fn cancelled_capture_stops_at_the_scanner_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("file"), "content").unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(
            capture_cancellable(directory.path(), true, &cancelled)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        cancelled.store(false, Ordering::Relaxed);
        assert!(capture_cancellable(directory.path(), true, &cancelled).is_ok());
    }

    #[test]
    fn symlink_targets_share_the_total_content_budget() {
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("four", root.path().join("first")).unwrap();
        std::os::unix::fs::symlink("x", root.path().join("second")).unwrap();
        let root_fd = File::open(root.path()).unwrap();
        // Start at the boundary as if preceding files consumed the budget;
        // exercise actual secure symlink capture without a 64 MiB fixture.
        let mut scan = Scan {
            root: &root_fd,
            started: Instant::now(),
            entries: BTreeMap::new(),
            stamps: BTreeMap::new(),
            raw: None,
            cancelled: None,
            scope: &CaptureScope::default(),
            bytes: MAX_TOTAL_BYTES - 4,
        };
        scan.walk(Path::new("first"), 1).unwrap();
        assert_eq!(scan.bytes, MAX_TOTAL_BYTES);
        let error = scan.walk(Path::new("second"), 1).unwrap_err();
        assert!(error.to_string().contains("64 MiB capture limit"));
    }
}
