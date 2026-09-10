//! Private staging cleanup retains identities, not one descriptor per directory.
use anyhow::{Context, Result, ensure};
use rustix::fs::{Mode, OFlags, ResolveFlags};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Component, Path, PathBuf},
};

pub(super) const RESOLVE: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_XDEV);

pub(super) struct StagingDirectory {
    directory: Option<tempfile::TempDir>,
    pub(super) root: File,
    directories: BTreeMap<PathBuf, (u64, u64)>,
}
impl StagingDirectory {
    pub(super) fn new() -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("demoncoder-hook-staging-")
            .tempdir()?;
        let root = File::open(directory.path())?;
        Ok(Self {
            directory: Some(directory),
            root,
            directories: BTreeMap::new(),
        })
    }
    #[cfg(test)]
    pub(super) fn path(&self) -> &Path {
        self.directory.as_ref().expect("staging still owned").path()
    }
    pub(super) fn record(&mut self, path: &Path, directory: &File) -> Result<()> {
        ensure!(
            !path.as_os_str().is_empty()
                && path.components().all(|c| matches!(c, Component::Normal(_))),
            "invalid staging directory name"
        );
        let metadata = directory.metadata()?;
        ensure!(metadata.is_dir(), "staging object is not a directory");
        ensure!(
            !self.directories.contains_key(path),
            "staging directory recorded twice"
        );
        self.directories
            .insert(path.to_owned(), (metadata.dev(), metadata.ino()));
        Ok(())
    }
    pub(super) fn open_directory(&self, path: &Path, access: OFlags) -> Result<File> {
        let identity = self
            .directories
            .get(path)
            .context("private staging directory was not recorded")?;
        let file: File = rustix::fs::openat2(
            &self.root,
            path,
            access | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
            RESOLVE,
        )?
        .into();
        let metadata = file.metadata()?;
        ensure!(
            (metadata.dev(), metadata.ino()) == *identity,
            "private staging directory identity changed"
        );
        Ok(file)
    }
    fn cleanup(&mut self) -> Result<()> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let named = directory.path().symlink_metadata()?;
        let pinned = self.root.metadata()?;
        ensure!(
            named.is_dir() && (named.dev(), named.ino()) == (pinned.dev(), pinned.ino()),
            "private staging root identity changed"
        );
        rustix::fs::fchmod(&self.root, Mode::from_raw_mode(0o700))?;
        // Parents sort before children. O_PATH can pin a mode-000 directory;
        // only ancestors, already restored, need search permission.
        for path in self.directories.keys() {
            let file = self.open_directory(path, OFlags::PATH)?;
            // This fixed proc reference follows our live, identity-checked pin,
            // never a source pathname or a package-supplied symlink.
            rustix::fs::chmodat(
                rustix::fs::CWD,
                format!("/proc/self/fd/{}", file.as_raw_fd()),
                Mode::from_raw_mode(0o700),
                rustix::fs::AtFlags::empty(),
            )?;
        }
        if let Some(directory) = self.directory.take() {
            directory
                .close()
                .context("remove private staging directory")?;
        }
        Ok(())
    }
}
impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if self.cleanup().is_err() {
            // Do not let TempDir retry an unverified replacement pathname.
            if let Some(directory) = self.directory.take() {
                let _retained_path = directory.keep();
            }
            // A closed diagnostic stream must not turn Drop into a second failure.
            let _diagnostic = writeln!(
                std::io::stderr().lock(),
                "warning: private hook staging cleanup failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::gate_snapshot::{GateReadSet, GateWorkspace};
    use std::{os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};

    fn create_directory(staging: &mut StagingDirectory, path: &str) {
        let path = Path::new(path);
        std::fs::create_dir(staging.path().join(path)).unwrap();
        let file = File::open(staging.path().join(path)).unwrap();
        staging.record(path, &file).unwrap();
    }

    #[test]
    fn mode_zero_nested_directories_and_root_are_cleaned_without_following_links() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("canary"), b"unchanged").unwrap();
        let outside_mode = outside.path().metadata().unwrap().mode();
        let mut staging = StagingDirectory::new().unwrap();
        for path in ["parent", "parent/child", "sibling"] {
            create_directory(&mut staging, path);
        }
        std::os::unix::fs::symlink(outside.path(), staging.path().join("parent/child/link"))
            .unwrap();
        for path in ["parent/child", "parent", "sibling"] {
            std::fs::set_permissions(
                staging.path().join(path),
                std::fs::Permissions::from_mode(0o000),
            )
            .unwrap();
        }
        rustix::fs::fchmod(&staging.root, Mode::empty()).unwrap();
        let path = staging.path().to_owned();
        drop(staging);
        assert!(!path.exists(), "mode-zero staging tree survived cleanup");
        assert_eq!(outside.path().metadata().unwrap().mode(), outside_mode);
        assert_eq!(
            std::fs::read(outside.path().join("canary")).unwrap(),
            b"unchanged"
        );
    }

    #[test]
    fn directory_replacement_and_symlink_cannot_rebind_metadata_or_cleanup() {
        let mut staging = StagingDirectory::new().unwrap();
        create_directory(&mut staging, "child");
        let child = staging.path().join("child");
        let moved = staging.path().join("moved");
        std::fs::rename(&child, &moved).unwrap();
        std::fs::create_dir(&child).unwrap();
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert!(
            staging
                .open_directory(Path::new("child"), OFlags::RDONLY)
                .is_err()
        );
        assert!(staging.cleanup().is_err());
        assert_eq!(child.metadata().unwrap().mode() & 0o777, 0o500);
        std::fs::remove_dir(&child).unwrap();
        std::os::unix::fs::symlink(&moved, &child).unwrap();
        assert!(
            staging
                .open_directory(Path::new("child"), OFlags::PATH)
                .is_err()
        );
        assert!(staging.cleanup().is_err());
        std::fs::remove_file(&child).unwrap();
        std::fs::rename(&moved, &child).unwrap();
        staging.cleanup().unwrap();
    }

    #[test]
    fn root_replacement_is_rejected_before_cleanup_changes_permissions() {
        let mut staging = StagingDirectory::new().unwrap();
        let path = staging.path().to_owned();
        let outside = tempfile::tempdir().unwrap();
        let moved = outside.path().join("original");
        std::fs::rename(&path, &moved).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
        assert!(staging.cleanup().is_err());
        assert_eq!(path.metadata().unwrap().mode() & 0o777, 0o500);
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(&moved, &path).unwrap();
        staging.cleanup().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn automatic_drop_preserves_replacements_after_identity_rejection() {
        for replace_root in [false, true] {
            let mut staging = StagingDirectory::new().unwrap();
            let path = staging.path().to_owned();
            let outside = tempfile::tempdir().unwrap();
            let original = outside.path().join("original");
            let replacement = if replace_root {
                path.clone()
            } else {
                create_directory(&mut staging, "child");
                path.join("child")
            };
            std::fs::rename(&replacement, &original).unwrap();
            std::fs::create_dir(&replacement).unwrap();
            std::fs::write(replacement.join("canary"), b"replacement untouched").unwrap();
            std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o500)).unwrap();
            drop(staging);
            assert_eq!(replacement.metadata().unwrap().mode() & 0o777, 0o500);
            assert_eq!(
                std::fs::read(replacement.join("canary")).unwrap(),
                b"replacement untouched"
            );
            assert!(
                original.exists(),
                "rejected cleanup removed the original tree"
            );
            assert!(
                path.exists(),
                "TempDir removed a rejected pathname after Drop"
            );
            std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::remove_file(replacement.join("canary")).unwrap();
            std::fs::remove_dir(&replacement).unwrap();
            if !replace_root {
                std::fs::remove_dir(path).unwrap();
            }
        }
    }

    #[test]
    fn metadata_failure_cleans_partially_prepared_restrictive_tree() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("file"), b"bytes").unwrap();
        let snapshot = GateWorkspace::open(source.path())
            .unwrap()
            .capture(&GateReadSet::default(), &AtomicBool::new(false))
            .unwrap();
        let entry = snapshot.entry("file").unwrap();
        let mut unsupported = entry.access().clone();
        unsupported.supported_attributes ^= 1u64 << 63;
        let path = std::cell::RefCell::new(PathBuf::new());
        let prepare = || -> Result<()> {
            let mut staging = StagingDirectory::new()?;
            *path.borrow_mut() = staging.path().to_owned();
            create_directory(&mut staging, "parent");
            create_directory(&mut staging, "parent/child");
            let file = File::create(staging.path().join("parent/child/file"))?;
            for directory in ["parent/child", "parent"] {
                std::fs::set_permissions(
                    staging.path().join(directory),
                    std::fs::Permissions::from_mode(0o000),
                )?;
            }
            rustix::fs::fchmod(&staging.root, Mode::empty())?;
            crate::workflow::workspace::access::restore_and_verify(
                &file,
                None,
                entry.mode(),
                &unsupported,
                &|| Ok(()),
            )
        };
        assert!(
            prepare()
                .unwrap_err()
                .to_string()
                .contains("statx semantics")
        );
        assert!(!path.borrow().exists(), "partial failed staging leaked");
    }
}
