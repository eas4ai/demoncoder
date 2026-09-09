//! The default coding view: normal reads and networking, confined file writes.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, ensure};
use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
use tokio::process::Command;

use crate::export_policy::private_roots;

const INSTRUCTION_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "RTK.md",
    "TILTH.md",
    "PARTNERSHIP.md",
    "BEST_PRACTICES.md",
];
// Changes to these tool stores go to session-owned overlays. Existing downloads
// remain readable; no cache hard link can carry a write back to an outside file.
const CACHE_PATHS: &[&str] = &[".cargo", ".cache", ".npm", ".bun/install/cache"];

pub(crate) struct DeveloperAccess {
    workspace: PathBuf,
    home: Option<PathBuf>,
    private: Vec<PathBuf>,
    instructions: Vec<PathBuf>,
    instruction_trees: Vec<PathBuf>,
    caches: Vec<Cache>,
    scratch: tempfile::TempDir,
    denied_file: tempfile::NamedTempFile,
    socket_filter: File,
}

struct Links {
    count: u64,
    paths: Vec<PathBuf>,
}

struct Cache {
    path: PathBuf,
    upper: PathBuf,
    work: PathBuf,
    overlay: bool,
}

/// Dropping the awaiting tool stops directory traversal as well as preventing
/// command launch. Blocking inspection never executes the task command itself.
struct InspectionGuard(Arc<AtomicBool>);
impl Drop for InspectionGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(crate) async fn inspect<T: Send + 'static>(
    work: impl FnOnce(&AtomicBool) -> Result<T> + Send + 'static,
) -> Result<T> {
    let guard = InspectionGuard(Arc::new(AtomicBool::new(false)));
    let cancelled = guard.0.clone();
    tokio::task::spawn_blocking(move || work(&cancelled))
        .await
        .context("join developer access inspection")?
}

fn checkpoint(cancelled: &AtomicBool) -> Result<()> {
    ensure!(
        !cancelled.load(Ordering::Relaxed),
        "developer access inspection cancelled"
    );
    Ok(())
}

impl DeveloperAccess {
    pub(crate) fn new(workspace: &Path, credential_paths: &[PathBuf]) -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .and_then(|path| path.canonicalize().ok());
        let mut private = private_roots(workspace, credential_paths, true);
        private.push(workspace.join(".demoncoder"));
        let mut instructions = Vec::new();
        let mut instruction_trees = Vec::new();
        let mut caches = Vec::new();
        if let Some(home) = &home {
            for directory in [".codex", ".claude"] {
                instructions.extend(
                    INSTRUCTION_FILES
                        .iter()
                        .map(|name| home.join(directory).join(name)),
                );
                instruction_trees.extend(
                    ["skills", "plugins"]
                        .iter()
                        .map(|name| home.join(directory).join(name)),
                );
            }
            caches.extend(CACHE_PATHS.iter().map(|path| home.join(path)));
        }
        private = private
            .into_iter()
            .map(std::path::absolute)
            .collect::<std::io::Result<_>>()?;
        // Keep lexical and physical names protected when a trusted setting uses
        // a symlink. A repository cannot add new read exceptions.
        for path in private.clone() {
            if let Ok(resolved) = path.canonicalize() {
                private.push(resolved);
            }
        }
        private.sort();
        private.dedup();
        instructions.retain(|path| {
            path.symlink_metadata()
                .is_ok_and(|meta| meta.is_file() && meta.nlink() == 1)
        });
        instruction_trees.retain(|path| path.is_dir());
        caches.retain(|path| {
            path.symlink_metadata()
                .is_ok_and(|meta| meta.is_dir() && !meta.is_symlink())
        });
        let scratch = tempfile::Builder::new()
            .prefix("demoncoder-confined-")
            .tempdir()?;
        for directory in ["cache", "share"] {
            std::fs::create_dir(scratch.path().join(directory))?;
        }
        let caches = caches
            .into_iter()
            .enumerate()
            .map(|(index, path)| {
                let upper = scratch.path().join(format!("cache-{index}"));
                let work = scratch.path().join(format!("work-{index}"));
                std::fs::create_dir(&upper)?;
                std::fs::create_dir(&work)?;
                // Probe with a command that cannot have task effects. Older kernels
                // or bubblewrap builds get an empty private cache, never host writes.
                let overlay = std::process::Command::new("/usr/bin/bwrap")
                    .args(["--unshare-all", "--ro-bind", "/", "/", "--overlay-src"])
                    .arg(&path)
                    .arg("--overlay")
                    .arg(&upper)
                    .arg(&work)
                    .arg(&path)
                    .arg("/usr/bin/true")
                    .env_clear()
                    .output()
                    .is_ok_and(|output| output.status.success());
                Ok(Cache {
                    path,
                    upper,
                    work,
                    overlay,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let denied_file = tempfile::NamedTempFile::new_in(scratch.path())?;
        denied_file
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o000))?;
        Ok(Self {
            workspace: workspace.to_path_buf(),
            home,
            private,
            instructions,
            instruction_trees,
            caches,
            scratch,
            denied_file,
            socket_filter: crate::socket_filter::file()?,
        })
    }

    fn protected(&self, path: &Path) -> bool {
        if self.instructions.iter().any(|allowed| path == allowed)
            || self
                .instruction_trees
                .iter()
                .any(|allowed| path.starts_with(allowed))
        {
            return false;
        }
        path.components()
            .any(|part| part.as_os_str() == ".demoncoder")
            || self.private.iter().any(|root| path.starts_with(root))
    }

    pub(crate) fn check_mutation(&self, path: &Path) -> Result<()> {
        ensure!(
            !path
                .components()
                .any(|part| part.as_os_str() == ".demoncoder")
                && !self.private.iter().any(|root| path.starts_with(root)),
            "cannot change protected settings or machine instructions: {path:?}"
        );
        Ok(())
    }

    pub(crate) async fn read(self: &Arc<Self>, root: Arc<File>, path: String) -> Result<File> {
        let access = self.clone();
        inspect(move |cancelled| access.read_file(&root, &path, cancelled)).await
    }

    fn read_file(&self, root: &File, path: &str, cancelled: &AtomicBool) -> Result<File> {
        ensure!(!path.is_empty() && path.len() <= 4096, "invalid read path");
        let file = File::from(
            openat2(
                root,
                path,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
                ResolveFlags::NO_MAGICLINKS,
            )
            .with_context(|| format!("open readable file {path:?}"))?,
        );
        let resolved = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
        ensure!(
            !self.protected(&resolved)
                && !resolved.starts_with("/proc")
                && !resolved.starts_with("/dev")
                && !resolved.starts_with("/sys"),
            "read is blocked for protected credentials or process state: {path:?}"
        );
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.nlink() > 0,
            "read requires a linked regular file: {path:?}"
        );
        if meta.nlink() > 1 {
            let private = inspect_links(&self.workspace, &mut BTreeMap::new(), cancelled)?;
            ensure!(
                !self
                    .private_links(&private, cancelled)?
                    .contains(&(meta.dev(), meta.ino())),
                "read is blocked for a credential alias: {path:?}"
            );
        }
        Ok(file)
    }

    fn private_links(
        &self,
        additional: &[PathBuf],
        cancelled: &AtomicBool,
    ) -> Result<BTreeSet<(u64, u64)>> {
        let mut linked = BTreeSet::new();
        for path in self.private.iter().chain(additional) {
            collect_private_links(path, &mut linked, &|path| self.protected(path), cancelled)?;
        }
        Ok(linked)
    }

    pub(crate) async fn command(
        self: &Arc<Self>,
        root: Arc<File>,
        workspace: PathBuf,
        script: String,
    ) -> Result<Command> {
        let access = self.clone();
        inspect(move |cancelled| access.build_command(&root, &workspace, &script, cancelled)).await
    }

    fn build_command(
        &self,
        root: &File,
        workspace: &Path,
        script: &str,
        cancelled: &AtomicBool,
    ) -> Result<Command> {
        let physical = std::fs::read_link(format!("/proc/self/fd/{}", root.as_raw_fd()))?;
        ensure!(
            physical == workspace,
            "workspace moved; reopen the session at its current path"
        );
        let mut links = BTreeMap::<(u64, u64), Links>::new();
        let private = inspect_links(workspace, &mut links, cancelled)?;
        let protected_inodes = self.private_links(&private, cancelled)?;

        // The fixed host launcher opens the policy, closes ambient descriptors,
        // and execs bwrap. Only the pinned root (stdin), output pipes and policy
        // cross this boundary. The task script stays an argument to inner Bash.
        // Opening through proc gives each launch an independent file offset.
        let mut command = Command::new("/bin/bash");
        command
            .args([
                "--noprofile",
                "--norc",
                "-c",
                r#"exec 3<"$1" || exit
shift
for task_fd_path in /proc/self/fd/*; do
    task_fd=${task_fd_path##*/}
    case "$task_fd" in
        0|1|2|3) ;;
        *) [[ "$task_fd" =~ ^[0-9]+$ ]] || exit 1
           exec {task_fd}>&- || exit ;;
    esac
done
exec "$@""#,
                "demoncoder-sandbox",
            ])
            .arg(format!(
                "/proc/{}/fd/{}",
                std::process::id(),
                self.socket_filter.as_raw_fd()
            ))
            .args(["/usr/bin/bwrap", "--seccomp", "3"]);
        command.args([
            "--unshare-all",
            "--share-net",
            "--die-with-parent",
            "--new-session",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
        ]);
        // Keep the real project path so tools, Git worktrees, and documentation
        // agree with the selected workspace. Other host paths stay read-only.
        command.arg("--bind-fd").arg("0").arg(workspace);
        command
            .arg("--bind")
            .arg(self.scratch.path())
            .arg(self.scratch.path());
        for cache in &self.caches {
            if cache.overlay {
                command
                    .arg("--overlay-src")
                    .arg(&cache.path)
                    .arg("--overlay")
                    .arg(&cache.upper)
                    .arg(&cache.work)
                    .arg(&cache.path);
            } else {
                command.arg("--bind").arg(&cache.upper).arg(&cache.path);
                // Cargo keeps executable shims beside its cache. A cold-cache
                // fallback must not hide the already installed toolchain.
                let bin = cache.path.join("bin");
                if cache.path.file_name().is_some_and(|name| name == ".cargo") && bin.is_dir() {
                    command.arg("--ro-bind").arg(&bin).arg(&bin);
                }
            }
        }
        let mut empty_directories = Vec::new();
        let mut private: Vec<_> = self.private.iter().chain(&private).collect();
        private.sort();
        private.dedup();
        for path in private {
            if let Ok(meta) = path.symlink_metadata() {
                if meta.is_dir() {
                    command.arg("--tmpfs").arg(path);
                    empty_directories.push(path);
                } else if meta.is_file() {
                    command
                        .arg("--ro-bind")
                        .arg(self.denied_file.path())
                        .arg(path);
                }
            }
        }
        for path in &self.instructions {
            command.arg("--ro-bind").arg(path).arg(path);
        }
        for path in &self.instruction_trees {
            command.arg("--ro-bind").arg(path).arg(path);
        }
        for (identity, group) in links {
            checkpoint(cancelled)?;
            let secret = protected_inodes.contains(&identity);
            if !secret && group.count == group.paths.len() as u64 {
                continue;
            }
            for path in group.paths {
                // A private inode remains private under another filename. An
                // ordinary outside alias is readable but cannot change its
                // outside sibling through the writable workspace mount.
                let source = if secret {
                    self.denied_file.path()
                } else {
                    &path
                };
                command.arg("--ro-bind").arg(source).arg(&path);
            }
        }
        for path in empty_directories {
            command.arg("--remount-ro").arg(path);
        }
        command.arg("--chdir").arg(workspace).arg("--clearenv");
        let path = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
        command.arg("--setenv").arg("PATH").arg(path);
        command
            .arg("--setenv")
            .arg("HOME")
            .arg(self.home.as_deref().unwrap_or(self.scratch.path()));
        command
            .arg("--setenv")
            .arg("TMPDIR")
            .arg(self.scratch.path());
        command
            .arg("--setenv")
            .arg("XDG_CACHE_HOME")
            .arg(self.scratch.path().join("cache"));
        command
            .arg("--setenv")
            .arg("XDG_DATA_HOME")
            .arg(self.scratch.path().join("share"));
        command.args([
            "--setenv",
            "LANG",
            "C.UTF-8",
            "--setenv",
            "RUSTUP_AUTO_INSTALL",
            "0",
        ]);
        command.args(["/bin/bash", "--noprofile", "--norc", "-c", script]);
        command.env_clear();
        Ok(command)
    }
}

fn inspect_links(
    root: &Path,
    links: &mut BTreeMap<(u64, u64), Links>,
    cancelled: &AtomicBool,
) -> Result<Vec<PathBuf>> {
    let mut directories = vec![root.to_path_buf()];
    let mut private = Vec::new();
    while let Some(directory) = directories.pop() {
        checkpoint(cancelled)?;
        for entry in std::fs::read_dir(&directory)
            .with_context(|| format!("inspect workspace directory {directory:?}"))?
        {
            checkpoint(cancelled)?;
            let path = entry?.path();
            let meta = path
                .symlink_metadata()
                .with_context(|| format!("inspect workspace entry {path:?}"))?;
            if meta.is_dir() {
                if path.file_name().is_some_and(|name| name == ".demoncoder") {
                    private.push(path);
                } else {
                    directories.push(path);
                }
            } else if meta.is_file() && meta.nlink() > 1 {
                let group = links
                    .entry((meta.dev(), meta.ino()))
                    .or_insert_with(|| Links {
                        count: meta.nlink(),
                        paths: Vec::new(),
                    });
                ensure!(
                    group.count == meta.nlink(),
                    "workspace links changed during inspection: {path:?}; retry"
                );
                group.paths.push(path);
            }
        }
    }
    Ok(private)
}

fn collect_private_links(
    root: &Path,
    linked: &mut BTreeSet<(u64, u64)>,
    protected: &impl Fn(&Path) -> bool,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        checkpoint(cancelled)?;
        if !protected(&path) {
            continue;
        }
        let meta = match path.symlink_metadata() {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("inspect protected path {path:?}"));
            }
        };
        if meta.is_dir() {
            for entry in std::fs::read_dir(&path)? {
                checkpoint(cancelled)?;
                pending.push(entry?.path());
            }
        } else if meta.is_file() && meta.nlink() > 1 {
            linked.insert((meta.dev(), meta.ino()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_preparation_yields_and_stops_without_starting_a_command() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("entry"), "fixture").unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
        let launched = Arc::new(AtomicBool::new(false));
        let task_launched = launched.clone();
        let task = tokio::spawn(async move {
            let result = inspect(move |cancelled| {
                let _ = started_tx.send(());
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                // Hold preparation open until the awaiting tool is cancelled.
                while !cancelled.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                let workspace_stopped =
                    inspect_links(root.path(), &mut BTreeMap::new(), cancelled).is_err();
                let private_stopped =
                    collect_private_links(root.path(), &mut BTreeSet::new(), &|_| true, cancelled)
                        .is_err();
                let _ = stopped_tx.send(workspace_stopped && private_stopped);
                Ok(())
            })
            .await;
            if result.is_ok() {
                task_launched.store(true, Ordering::Relaxed);
            }
        });
        tokio::time::timeout(std::time::Duration::from_millis(500), started_rx)
            .await
            .unwrap()
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(500), stopped_rx)
                .await
                .unwrap()
                .unwrap(),
            "abandoned inspection continued traversing"
        );
        assert!(
            !launched.load(Ordering::Relaxed),
            "cancelled preparation reached command launch"
        );
    }

    #[tokio::test]
    async fn cold_cache_keeps_installed_tools_and_does_not_write_the_host_cache() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("project");
        let cache = root.path().join("home/.cargo");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir_all(cache.join("bin")).unwrap();
        let executable = cache.join("bin/cargo");
        std::fs::write(&executable, "#!/bin/sh\nprintf INSTALLED-TOOL\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut access = DeveloperAccess::new(&workspace, &[]).unwrap();
        let upper = access.scratch.path().join("cold-upper");
        let work = access.scratch.path().join("cold-work");
        std::fs::create_dir(&upper).unwrap();
        std::fs::create_dir(&work).unwrap();
        access.caches = vec![Cache {
            path: cache.clone(),
            upper,
            work,
            overlay: false,
        }];
        let directory = File::open(&workspace).unwrap();
        let script = format!(
            "set -e; '{}'; mkdir -p '{}/registry'; printf cached > '{}/registry/result'",
            executable.display(),
            cache.display(),
            cache.display()
        );
        let output = access
            .build_command(&directory, &workspace, &script, &AtomicBool::new(false))
            .unwrap()
            .stdin(Stdio::from(directory.try_clone().unwrap()))
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"INSTALLED-TOOL");
        assert!(!cache.join("registry/result").exists());
        assert_eq!(
            std::fs::read_to_string(access.caches[0].upper.join("registry/result")).unwrap(),
            "cached"
        );
    }
}
