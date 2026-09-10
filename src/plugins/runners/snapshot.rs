//! Private materialization of retained gate evidence, never a live workspace copy.
use super::staging::{RESOLVE, StagingDirectory};
use crate::plugins::gate_snapshot::EntryKind;
use crate::plugins::gate_snapshot::GateSnapshot;
use anyhow::{Result, ensure};
use rustix::fs::{Mode, OFlags};
use std::{
    fs::File,
    io::Write,
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub(crate) struct SnapshotMount {
    _directory: StagingDirectory,
    pub(crate) root: File,
}
impl SnapshotMount {
    pub(crate) fn materialize(snapshot: &GateSnapshot, cancelled: &AtomicBool) -> Result<Self> {
        let started = Instant::now();
        let checkpoint = || {
            ensure!(
                !cancelled.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(10),
                "snapshot materialization cancelled or timed out"
            );
            Ok(())
        };
        checkpoint()?;
        let directory = StagingDirectory::new()?;
        let root = directory.root.try_clone()?;
        let mut mount = Self {
            _directory: directory,
            root,
        };
        let root = &mount.root;
        let mut directories = Vec::new();
        // Snapshot names are canonical, but validate again at the write boundary.
        for (name, entry) in snapshot.entries() {
            checkpoint()?;
            if name == "." {
                continue;
            }
            let path = Path::new(name);
            ensure!(
                path.components().all(|c| matches!(c, Component::Normal(_))),
                "invalid snapshot object name"
            );
            let parent: File = rustix::fs::openat2(
                root,
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                RESOLVE,
            )?
            .into();
            let leaf = path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("missing snapshot leaf"))?;
            let file: File = match entry.kind() {
                EntryKind::Directory => {
                    rustix::fs::mkdirat(&parent, leaf, Mode::from_raw_mode(0o700))?;
                    rustix::fs::openat2(
                        &parent,
                        leaf,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                        Mode::empty(),
                        RESOLVE,
                    )?
                    .into()
                }
                EntryKind::File => {
                    let mut file: File = rustix::fs::openat2(
                        &parent,
                        leaf,
                        OFlags::CREATE | OFlags::EXCL | OFlags::RDWR | OFlags::CLOEXEC,
                        Mode::from_raw_mode(0o600),
                        RESOLVE,
                    )?
                    .into();
                    for bytes in entry.bytes().chunks(65536) {
                        checkpoint()?;
                        file.write_all(bytes)?;
                    }
                    file
                }
                EntryKind::Symlink => {
                    use std::os::unix::ffi::OsStrExt;
                    rustix::fs::symlinkat(
                        std::ffi::OsStr::from_bytes(entry.bytes()),
                        &parent,
                        leaf,
                    )?;
                    rustix::fs::openat2(
                        &parent,
                        leaf,
                        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                        RESOLVE,
                    )?
                    .into()
                }
            };
            if matches!(entry.kind(), EntryKind::Directory) {
                mount._directory.record(path, &file)?;
                directories.push((name, entry));
            } else {
                let link = matches!(entry.kind(), EntryKind::Symlink).then_some((&parent, leaf));
                crate::workflow::workspace::access::restore_and_verify(
                    &file,
                    link,
                    entry.mode(),
                    entry.access(),
                    &checkpoint,
                )?;
            }
        }
        // Only paths survive creation. Reopen one directory at a time, children
        // before parents, while every ancestor still has its staging permissions.
        for (name, entry) in directories.into_iter().rev() {
            checkpoint()?;
            let file = mount
                ._directory
                .open_directory(Path::new(name), OFlags::RDONLY)?;
            crate::workflow::workspace::access::restore_and_verify(
                &file,
                None,
                entry.mode(),
                entry.access(),
                &checkpoint,
            )?;
        }
        let entry = snapshot
            .entry(".")
            .ok_or_else(|| anyhow::anyhow!("snapshot root metadata is unavailable"))?;
        crate::workflow::workspace::access::restore_and_verify(
            root,
            None,
            entry.mode(),
            entry.access(),
            &checkpoint,
        )?;
        Ok(mount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::gate_snapshot::{GateReadSet, GateWorkspace};
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };

    #[test]
    fn retained_dirty_bytes_modes_and_absence_are_materialized_without_live_reads() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("untracked"), b"dirty bytes").unwrap();
        std::fs::set_permissions(
            source.path().join("untracked"),
            std::fs::Permissions::from_mode(0o751),
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        let snapshot = GateWorkspace::open(source.path())
            .unwrap()
            .capture(&GateReadSet::default(), &cancelled)
            .unwrap();
        std::fs::remove_file(source.path().join("untracked")).unwrap();
        std::fs::write(source.path().join("later"), b"not captured").unwrap();
        let mount = SnapshotMount::materialize(&snapshot, &cancelled).unwrap();
        let path = mount._directory.path().join("untracked");
        let mut bytes = Vec::new();
        File::open(&path).unwrap().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"dirty bytes");
        assert_eq!(path.metadata().unwrap().mode() & 0o7777, 0o751);
        assert_eq!(
            path.metadata().unwrap().uid(),
            snapshot.entry("untracked").unwrap().access().uid
        );
        assert!(!mount._directory.path().join("later").exists());
        assert!(mount.root.metadata().unwrap().is_dir());
    }
    #[test]
    fn directory_and_root_acl_modes_and_symlinks_restore_and_cleanup() {
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("dir")).unwrap();
        std::fs::write(source.path().join("dir/file"), b"bytes").unwrap();
        std::os::unix::fs::symlink("dir/file", source.path().join("link")).unwrap();
        let mut acl = 2u32.to_le_bytes().to_vec();
        for (tag, permission, id) in [
            (1u16, 7u16, u32::MAX),
            (2, 4, rustix::process::getuid().as_raw() + 1),
            (4, 5, u32::MAX),
            (16, 5, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            acl.extend(tag.to_le_bytes());
            acl.extend(permission.to_le_bytes());
            acl.extend(id.to_le_bytes());
        }
        let directory = File::open(source.path().join("dir")).unwrap();
        rustix::fs::fsetxattr(
            &directory,
            "system.posix_acl_access",
            &acl,
            rustix::fs::XattrFlags::empty(),
        )
        .unwrap();
        rustix::fs::fsetxattr(
            &directory,
            "system.posix_acl_default",
            &acl,
            rustix::fs::XattrFlags::empty(),
        )
        .unwrap();
        std::fs::set_permissions(source.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let cancelled = AtomicBool::new(false);
        let snapshot = GateWorkspace::open(source.path())
            .unwrap()
            .capture(&GateReadSet::default(), &cancelled)
            .unwrap();
        let mount = SnapshotMount::materialize(&snapshot, &cancelled).unwrap();
        assert_eq!(mount.root.metadata().unwrap().mode() & 0o7777, 0o500);
        assert_eq!(
            std::fs::read_link(mount._directory.path().join("link")).unwrap(),
            Path::new("dir/file")
        );
        let path = mount._directory.path().to_owned();
        drop(mount);
        assert!(
            !path.exists(),
            "readonly staging directories leaked during cleanup"
        );
        std::fs::set_permissions(source.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn unsupported_metadata_semantics_hold_and_partial_staging_is_cleaned() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("file"), b"bytes").unwrap();
        let cancelled = AtomicBool::new(false);
        let snapshot = GateWorkspace::open(source.path())
            .unwrap()
            .capture(&GateReadSet::default(), &cancelled)
            .unwrap();
        let entry = snapshot.entry("file").unwrap();
        let target = tempfile::tempfile().unwrap();
        let mut unsupported = entry.access().clone();
        unsupported.supported_attributes ^= 1u64 << 63;
        let error = crate::workflow::workspace::access::restore_and_verify(
            &target,
            None,
            entry.mode(),
            &unsupported,
            &|| Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("statx semantics"));
        if !rustix::process::geteuid().is_root() {
            let mut owner = entry.access().clone();
            owner.uid = owner.uid.checked_add(1).unwrap();
            let error = crate::workflow::workspace::access::restore_and_verify(
                &target,
                None,
                entry.mode(),
                &owner,
                &|| Ok(()),
            )
            .unwrap_err();
            assert!(error.to_string().contains("ownership"));
        }
        let cancelled = AtomicBool::new(true);
        assert!(
            SnapshotMount::materialize(&snapshot, &cancelled)
                .err()
                .unwrap()
                .to_string()
                .contains("cancelled")
        );
    }
}
