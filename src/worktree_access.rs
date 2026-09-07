//! Child Bash gets a fresh filesystem containing only system runtimes and its worktree.
use anyhow::{Result, ensure};
use std::{
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
use tokio::process::Command;

const RUNTIME_PATHS: &[&str] = &[
    "/usr/bin",
    "/usr/lib",
    "/usr/lib64",
    "/bin",
    "/lib",
    "/lib64",
    "/etc/ld.so.cache",
];

pub(crate) struct WorktreeAccess {
    credentials: Vec<PathBuf>,
    socket_filter: File,
    denied: tempfile::NamedTempFile,
}
impl WorktreeAccess {
    pub(crate) fn new(workspace: &Path, credentials: &[PathBuf]) -> Result<Self> {
        let mut protected = Vec::new();
        for path in credentials {
            let path = std::path::absolute(path)?;
            protected.push(path.clone());
            if let Ok(physical) = path.canonicalize() {
                protected.push(physical);
            }
        }
        ensure!(
            !protected.iter().any(|p| workspace.starts_with(p)),
            "worktree overlaps a credential store"
        );
        ensure!(
            !protected
                .iter()
                .any(|private| RUNTIME_PATHS.iter().any(|runtime| {
                    let runtime = Path::new(runtime);
                    private.starts_with(runtime) || runtime.starts_with(private)
                })),
            "credential store overlaps the child system runtime"
        );
        let denied = tempfile::NamedTempFile::new()?;
        denied
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o000))?;
        Ok(Self {
            credentials: protected,
            socket_filter: crate::socket_filter::file()?,
            denied,
        })
    }
    pub(crate) fn check_path(&self, path: &Path) -> Result<()> {
        ensure!(
            !self.credentials.iter().any(|p| path.starts_with(p)),
            "worktree tools cannot access credentials"
        );
        Ok(())
    }
    pub(crate) async fn command(
        self: &Arc<Self>,
        root: Arc<File>,
        workspace: PathBuf,
        script: String,
    ) -> Result<Command> {
        let access = self.clone();
        crate::developer_access::inspect(move |cancelled| {
            access.build_command(&root, &workspace, &script, cancelled)
        })
        .await
    }
    fn build_command(
        &self,
        root: &File,
        workspace: &Path,
        script: &str,
        cancelled: &AtomicBool,
    ) -> Result<Command> {
        ensure!(
            std::fs::read_link(format!("/proc/self/fd/{}", root.as_raw_fd()))? == workspace,
            "worktree moved; reopen it"
        );
        let masks = self.inspect(workspace, cancelled)?;
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
                "demoncoder-worktree",
            ])
            .arg(format!(
                "/proc/{}/fd/{}",
                std::process::id(),
                self.socket_filter.as_raw_fd()
            ))
            .args([
                "/usr/bin/bwrap",
                "--seccomp",
                "3",
                "--unshare-all",
                "--die-with-parent",
                "--new-session",
            ]);
        // Deliberately do not bind /, /home, /etc, or inherited PATH entries.
        for &path in RUNTIME_PATHS {
            if Path::new(path).exists() {
                command.args(["--ro-bind", path, path]);
            }
        }
        command.args(["--proc", "/proc", "--dev", "/dev", "--dir", "/tmp"]);
        command.arg("--bind-fd").arg("0").arg(workspace);
        for (path, directory) in masks {
            if directory {
                command
                    .arg("--tmpfs")
                    .arg(&path)
                    .arg("--remount-ro")
                    .arg(&path);
            } else {
                command.arg("--ro-bind").arg(self.denied.path()).arg(&path);
            }
        }
        command.args([
            "--remount-ro",
            "/",
            "--remount-ro",
            "/dev",
            "--remount-ro",
            "/proc",
        ]);
        command.arg("--chdir").arg(workspace).args([
            "--clearenv",
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--setenv",
            "LANG",
            "C.UTF-8",
        ]);
        command
            .arg("--setenv")
            .arg("HOME")
            .arg(workspace)
            .arg("--setenv")
            .arg("TMPDIR")
            .arg(workspace);
        command
            .args(["/bin/bash", "--noprofile", "--norc", "-c", script])
            .env_clear();
        Ok(command)
    }
    fn inspect(&self, workspace: &Path, cancelled: &AtomicBool) -> Result<Vec<(PathBuf, bool)>> {
        let mut pending = vec![workspace.to_path_buf()];
        let mut masks = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory)? {
                ensure!(
                    !cancelled.load(Ordering::Relaxed),
                    "worktree inspection cancelled"
                );
                let path = entry?.path();
                let meta = path.symlink_metadata()?;
                let protected = path
                    .file_name()
                    .is_some_and(|n| n == ".git" || n == ".demoncoder")
                    || self.credentials.iter().any(|p| path.starts_with(p));
                if protected {
                    ensure!(!meta.is_symlink(), "protected worktree entry is a symlink");
                    masks.push((path, meta.is_dir()));
                } else if meta.is_dir() {
                    pending.push(path);
                } else if meta.is_file() {
                    ensure!(
                        meta.nlink() == 1,
                        "worktree Bash refuses hard links: {path:?}"
                    );
                }
            }
        }
        Ok(masks)
    }
}
