//! Linux descriptor-relative, bounded capture. No consumer reopens the source.
use super::types::{InspectionReport, SnapshotDirectory, SnapshotFile, SourceIdentity};
use anyhow::{Context, Result, ensure};
use rustix::fs::{Dir, Mode, OFlags, ResolveFlags, openat2};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PATH_DEPTH: usize = 64;
pub(super) struct Snapshot {
    pub source: SourceIdentity,
    pub digest: String,
    pub files: BTreeMap<String, SnapshotFile>,
    pub directories: BTreeMap<String, SnapshotDirectory>,
    pub report: InspectionReport,
    private: Vec<PathBuf>,
}
pub(super) fn canonical_name(path: &str) -> Result<String> {
    ensure!(
        !path.is_empty() && !path.contains('\\') && !path.contains('\0'),
        "invalid package path"
    );
    let mut names = Vec::new();
    for part in Path::new(path).components() {
        match part {
            Component::CurDir => {}
            Component::Normal(n) => names.push(n.to_str().context("package paths must be UTF-8")?),
            _ => anyhow::bail!("package path escapes root: {path}"),
        }
    }
    ensure!(!names.is_empty(), "empty package path");
    ensure!(
        names.len() <= MAX_PATH_DEPTH,
        "package path exceeds {MAX_PATH_DEPTH} levels"
    );
    Ok(names.join("/"))
}
fn open(root: &File, path: &Path) -> Result<File> {
    Ok(openat2(
        root,
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .with_context(|| format!("cannot safely open package path {}", path.display()))?
    .into())
}
fn signature(m: &Metadata) -> (u64, u64, u64, u32, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mode(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
pub(super) fn capture(source: &Path) -> Result<Snapshot> {
    let canonical = source
        .canonicalize()
        .context("resolve selected package root")?;
    let private = protected_paths(&canonical)?;
    let selected = std::fs::symlink_metadata(&canonical)?;
    let root: File = openat2(
        rustix::fs::CWD,
        &canonical,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NONBLOCK,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .context("pin selected package root without links")?
    .into();
    let before = root.metadata()?;
    ensure!(before.is_dir(), "package source must be a directory");
    ensure!(
        signature(&before) == signature(&selected),
        "selected package root replaced before capture"
    );
    let mut snapshot = Snapshot {
        source: SourceIdentity {
            canonical_root: canonical.clone(),
            device: before.dev(),
            inode: before.ino(),
        },
        digest: String::new(),
        files: BTreeMap::new(),
        directories: BTreeMap::new(),
        report: InspectionReport::default(),
        private,
    };
    let mut count = 0;
    let mut total = 0;
    let mut signatures = BTreeMap::new();
    walk(
        &root,
        &root,
        "",
        &mut snapshot,
        &mut count,
        &mut total,
        &mut signatures,
    )?;
    verify_source(&root, &canonical, &before, &signatures)?;
    let mut hash = Sha256::new();
    hash.update(b"demoncoder-plugin-snapshot-v2\0");
    for (name, directory) in &snapshot.directories {
        hash.update(b"directory\0");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(directory.mode.to_le_bytes());
    }
    for (name, file) in &snapshot.files {
        hash.update(b"file\0");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(file.mode.to_le_bytes());
        hash.update((file.bytes.len() as u64).to_le_bytes());
        hash.update(&file.bytes);
    }
    snapshot.digest = format!("sha256:{:x}", hash.finalize());
    Ok(snapshot)
}
type Signature = (u64, u64, u64, u32, i64, i64, i64, i64);
fn walk(
    root: &File,
    directory: &File,
    prefix: &str,
    snapshot: &mut Snapshot,
    count: &mut usize,
    total: &mut usize,
    signatures: &mut BTreeMap<String, Signature>,
) -> Result<()> {
    let before = directory.metadata()?;
    for entry in Dir::read_from(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .context("package paths must be UTF-8")?;
        if name == "." || name == ".." {
            continue;
        }
        *count += 1;
        ensure!(
            *count <= MAX_ENTRIES,
            "package exceeds {MAX_ENTRIES} entries"
        );
        let relative = canonical_name(&if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        })?;
        if name == ".git"
            || crate::export_policy::private_path(Path::new(&relative))
            || snapshot.private.iter().any(|p| {
                snapshot
                    .source
                    .canonical_root
                    .join(&relative)
                    .starts_with(p)
            })
        {
            snapshot.report.exclusions.push(relative);
            continue;
        }
        let file = open(root, Path::new(&relative))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_dir() || metadata.is_file(),
            "special package file rejected: {relative}"
        );
        if metadata.is_dir() {
            ensure!(
                snapshot
                    .directories
                    .insert(
                        relative.clone(),
                        SnapshotDirectory {
                            mode: metadata.mode() & 0o7777
                        }
                    )
                    .is_none(),
                "duplicate package directory: {relative}"
            );
            walk(root, &file, &relative, snapshot, count, total, signatures)?;
        } else {
            ensure!(
                metadata.nlink() == 1,
                "hard-linked package file rejected: {relative}"
            );
            ensure!(
                metadata.len() <= MAX_FILE_BYTES as u64,
                "package file exceeds {MAX_FILE_BYTES} bytes: {relative}"
            );
            let mut bytes = Vec::new();
            (&file)
                .take((MAX_FILE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= MAX_FILE_BYTES,
                "package file exceeds {MAX_FILE_BYTES} bytes: {relative}"
            );
            *total += bytes.len();
            ensure!(
                *total <= MAX_TOTAL_BYTES,
                "package exceeds {MAX_TOTAL_BYTES} total bytes"
            );
            ensure!(
                signature(&file.metadata()?) == signature(&metadata),
                "package file changed during capture: {relative}"
            );
            let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
            ensure!(
                snapshot
                    .files
                    .insert(
                        relative.clone(),
                        SnapshotFile {
                            bytes,
                            mode: metadata.mode() & 0o7777,
                            digest
                        }
                    )
                    .is_none(),
                "duplicate package file: {relative}"
            );
        }
        signatures.insert(relative, signature(&metadata));
    }
    ensure!(
        signature(&directory.metadata()?) == signature(&before),
        "package directory changed during capture"
    );
    Ok(())
}

fn verify_source(
    root: &File,
    canonical: &Path,
    before: &Metadata,
    signatures: &BTreeMap<String, Signature>,
) -> Result<()> {
    for (name, expected) in signatures {
        ensure!(
            signature(&open(root, Path::new(name))?.metadata()?) == *expected,
            "package source changed during capture: {name}"
        );
    }
    ensure!(
        signature(&root.metadata()?) == signature(before),
        "package root changed during capture"
    );
    let after = std::fs::symlink_metadata(canonical)?;
    ensure!(
        after.dev() == before.dev() && after.ino() == before.ino(),
        "package source root replaced during capture"
    );
    Ok(())
}

fn installed_subtree(root: &Path, private: &Path) -> bool {
    matches!(
        private.file_name().and_then(|n| n.to_str()),
        Some(".codex" | ".claude" | ".demoncoder")
    ) && root
        .strip_prefix(private)
        .ok()
        .and_then(|r| r.components().next())
        .is_some_and(|c| matches!(c,Component::Normal(n) if n=="plugins" || n=="skills"))
}
fn protected_paths(root: &Path) -> Result<Vec<PathBuf>> {
    // A selected package inside an installed plugin/skill tree is public source;
    // this does not open the surrounding backend's settings or credential store.
    for ancestor in root.ancestors() {
        for private in crate::export_policy::PRIVATE_PATHS {
            ensure!(
                !ancestor.ends_with(private) || installed_subtree(root, ancestor),
                "selected package root is private"
            );
        }
    }
    let mut result = Vec::new();
    for private in crate::export_policy::private_roots(root, &[], true) {
        let lexical = std::path::absolute(&private)?;
        let mut candidates = vec![lexical];
        match private.canonicalize() {
            Ok(p) => candidates.push(p),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        for private in candidates {
            ensure!(
                !root.starts_with(&private) || installed_subtree(root, &private),
                "selected package overlaps a private credential root"
            );
            if private.starts_with(root) {
                result.push(private);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_replacement_at_validation_is_rejected() {
        let parent = tempfile::tempdir().unwrap();
        let source = parent.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let root = File::open(&source).unwrap();
        let before = root.metadata().unwrap();
        assert!(verify_source(&root, &source, &before, &BTreeMap::new()).is_ok());
        std::fs::rename(&source, parent.path().join("original")).unwrap();
        std::fs::create_dir(&source).unwrap();
        assert!(verify_source(&root, &source, &before, &BTreeMap::new()).is_err());
    }
    #[test]
    fn file_edit_at_validation_is_rejected_then_fresh_capture_succeeds() {
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("script");
        std::fs::write(&path, "before").unwrap();
        let root = File::open(source.path()).unwrap();
        let before = root.metadata().unwrap();
        let signatures = BTreeMap::from([(
            "script".into(),
            signature(&std::fs::metadata(&path).unwrap()),
        )]);
        assert!(verify_source(&root, source.path(), &before, &signatures).is_ok());
        std::fs::write(&path, "changed").unwrap();
        assert!(verify_source(&root, source.path(), &before, &signatures).is_err());
        assert_eq!(
            capture(source.path()).unwrap().files["script"].bytes(),
            b"changed"
        );
    }
    #[test]
    fn directory_mode_mutation_is_rejected_before_snapshot_admission() {
        use std::os::unix::fs::PermissionsExt;
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("empty");
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = File::open(source.path()).unwrap();
        let before = root.metadata().unwrap();
        let signatures = BTreeMap::from([(
            "empty".into(),
            signature(&std::fs::metadata(&path).unwrap()),
        )]);
        assert!(verify_source(&root, source.path(), &before, &signatures).is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(verify_source(&root, source.path(), &before, &signatures).is_err());
        assert_eq!(
            capture(source.path()).unwrap().directories["empty"].mode(),
            0o755
        );
    }
}
