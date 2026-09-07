//! Bounded, read-only Linux workspace capture. No Git commands, filters or hooks
//! run. Only the root `.git` entry is excluded; Git administrative changes are
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
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{Dir, Mode, OFlags, ResolveFlags, openat2, readlinkat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    root_device: u64,
    root_inode: u64,
    entries: BTreeMap<String, Entry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    kind: Kind,
    mode: u32,
    bytes: u64,
    hash: String,
    text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Kind {
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
    bytes: u64,
}

impl Scan<'_> {
    fn checkpoint(&self) -> Result<()> {
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

/// Capture all entries below a real directory, including ignored/untracked files.
/// Refuses symlink roots, descendant mounts, hard-linked regular files, special
/// files, non-UTF-8 names and oversized trees rather than silently skipping them.
pub fn capture(root: &Path) -> Result<Snapshot> {
    let started = Instant::now();
    let open_root = || -> Result<File> {
        Ok(openat2(rustix::fs::CWD, root, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW, Mode::empty(), ResolveFlags::NO_SYMLINKS)
            .context("cannot securely open workspace root; select a real directory without symlink ancestors")?.into())
    };
    let root_fd = open_root()?;
    let root_stamp = stamp(&root_fd)?;
    let scan = || -> Result<Scan<'_>> {
        let mut scan = Scan {
            root: &root_fd,
            started,
            entries: BTreeMap::new(),
            stamps: BTreeMap::new(),
            bytes: 0,
        };
        scan.walk(Path::new("."), 0)?;
        Ok(scan)
    };
    let first = scan()?;
    let second = scan()?;
    ensure!(
        first.entries == second.entries
            && first.stamps == second.stamps
            && stamp(&root_fd)? == root_stamp
            && stamp(&open_root()?)? == root_stamp,
        "workspace changed during capture; stop concurrent edits and retry"
    );
    let mut snapshot = Snapshot {
        digest: String::new(),
        root_device: root_stamp.device,
        root_inode: root_stamp.inode,
        entries: second.entries,
    };
    // Structured serialization supplies unambiguous length/delimiter encoding;
    // BTreeMap iteration provides deterministic ordering independent of readdir.
    snapshot.digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&snapshot)?));
    Ok(snapshot)
}

fn append(output: &mut String, text: &str) -> Result<()> {
    ensure!(
        output.len().saturating_add(text.len()) <= MAX_EVIDENCE_BYTES,
        "review evidence exceeds 1 MiB; review is blocked, narrow the task workspace before starting a new baseline"
    );
    output.push_str(text);
    Ok(())
}

fn describe(output: &mut String, side: &str, value: Option<&Entry>, changed: bool) -> Result<()> {
    let Some(value) = value else {
        return append(output, &format!("{side}: absent\n"));
    };
    append(
        output,
        &format!(
            "{side}: {:?}, mode {:o}, {} bytes, sha256 {}\n",
            value.kind, value.mode, value.bytes, value.hash
        ),
    )?;
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

/// Complete old/new changed text plus retained baseline text as review context.
/// Baseline includes pre-existing developer edits and untracked/ignored files;
/// no Git tracked/clean claim is made. Oversized or changed binary evidence fails.
pub fn review_evidence(before: &Snapshot, after: &Snapshot) -> Result<String> {
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
    let names: std::collections::BTreeSet<_> =
        before.entries.keys().chain(after.entries.keys()).collect();
    for name in names {
        let old = before.entries.get(name);
        let new = after.entries.get(name);
        let changed = old != new;
        append(
            &mut output,
            &format!(
                "\nPath {}: {}\n",
                serde_json::to_string(name)?,
                if changed {
                    "changed since baseline"
                } else {
                    "pre-existing baseline entry retained as context"
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
        } else {
            describe(&mut output, "RETAINED", new, false)?;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            bytes: MAX_TOTAL_BYTES - 4,
        };
        scan.walk(Path::new("first"), 1).unwrap();
        assert_eq!(scan.bytes, MAX_TOTAL_BYTES);
        let error = scan.walk(Path::new("second"), 1).unwrap_err();
        assert!(error.to_string().contains("64 MiB capture limit"));
    }
}
