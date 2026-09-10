//! Materialize immutable imported code, without consulting its original path.
use super::staging::{RESOLVE, StagingDirectory};
use crate::plugins::Package;
use anyhow::{Result, ensure};
use rustix::fs::{Mode, OFlags};
use std::{
    fs::File,
    io::Write,
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) struct PackageMount {
    _directory: StagingDirectory,
    pub(super) root: File,
}
impl PackageMount {
    pub(super) fn materialize(package: &Package, cancelled: &AtomicBool) -> Result<Self> {
        let directory = StagingDirectory::new()?;
        let root = directory.root.try_clone()?;
        let mut mount = Self {
            _directory: directory,
            root,
        };
        let root = &mount.root;
        for name in package.directories().keys() {
            ensure!(
                !cancelled.load(Ordering::Relaxed),
                "code materialization cancelled"
            );
            if name == "." {
                continue;
            }
            let path = checked_path(name)?;
            let parent = open_parent(root, path)?;
            rustix::fs::mkdirat(
                &parent,
                path.file_name().expect("checked leaf"),
                Mode::from_raw_mode(0o700),
            )?;
            let file: File = rustix::fs::openat2(
                &parent,
                path.file_name().expect("checked leaf"),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                RESOLVE,
            )?
            .into();
            mount._directory.record(path, &file)?;
        }
        for (name, source) in package.files() {
            let path = checked_path(name)?;
            let parent = open_parent(root, path)?;
            let mut file: File = rustix::fs::openat2(
                &parent,
                path.file_name().expect("checked leaf"),
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
                RESOLVE,
            )?
            .into();
            for bytes in source.bytes().chunks(65536) {
                ensure!(
                    !cancelled.load(Ordering::Relaxed),
                    "code materialization cancelled"
                );
                file.write_all(bytes)?;
            }
            // Imported code has no authority to add set-id bits or capabilities.
            rustix::fs::fchmod(&file, Mode::from_raw_mode(source.mode() & 0o777))?;
        }
        for (name, source) in package.directories().iter().rev() {
            let file = if name == "." {
                root.try_clone()?
            } else {
                mount
                    ._directory
                    .open_directory(checked_path(name)?, OFlags::RDONLY)?
            };
            rustix::fs::fchmod(&file, Mode::from_raw_mode(source.mode() & 0o777))?;
        }
        Ok(mount)
    }
}
pub(super) fn checked_path(name: &str) -> Result<&Path> {
    let path = Path::new(name);
    ensure!(
        !name.is_empty()
            && name.len() <= 4096
            && !name.contains(['\\', '\0'])
            && name.split('/').all(|p| !matches!(p, "" | "." | ".."))
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "command requires a canonical relative path"
    );
    Ok(path)
}
fn open_parent(root: &File, path: &Path) -> Result<File> {
    Ok(rustix::fs::openat2(
        root,
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        RESOLVE,
    )?
    .into())
}
