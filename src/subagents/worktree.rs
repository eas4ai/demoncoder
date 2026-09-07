//! Parent-owned Git worktrees and exact, validated content integration.
//! The caller records intent before either mutating operation. Errors can leave
//! retained worktrees/objects or a partly applied result; never imply rollback.
//! Mutation futures yield between entries and 64 KiB writes; cancellation never
//! detaches writes. Read-only capture workers stop at scanner checkpoints. Like
//! the filesystem tools, these bounds assume ordinary kernel syscall completion;
//! an uninterruptible filesystem syscall is not a hard real-time guarantee.
use super::state::{AssignmentRequest, WorktreeIdentity, valid_content_path};
use crate::workflow::workspace::{self, Kind, Snapshot};
use anyhow::{Context, Result, ensure};
use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

const LIMIT: usize = 192 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(60);

struct GitGroup(Option<rustix::process::Pid>);
impl GitGroup {
    fn stop(&mut self) -> Result<()> {
        if let Some(pid) = self.0 {
            match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => self.0 = None,
                Err(error) => return Err(error).context("stopping owned Git process group"),
            }
        }
        Ok(())
    }
}
impl Drop for GitGroup {
    fn drop(&mut self) {
        // Cancellation revokes subprocesses, not retained assignment artifacts.
        let _ = self.stop();
    }
}

struct CaptureCancellation {
    flag: Arc<AtomicBool>,
    worker: tokio::task::AbortHandle,
}
impl Drop for CaptureCancellation {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::Relaxed);
        self.worker.abort();
    }
}

async fn capture_background(
    root: &Path,
    raw: bool,
) -> Result<(Snapshot, BTreeMap<String, Vec<u8>>)> {
    let root = root.to_owned();
    let flag = Arc::new(AtomicBool::new(false));
    let worker_flag = flag.clone();
    let worker = tokio::task::spawn_blocking(move || {
        workspace::capture_cancellable(&root, raw, &worker_flag)
    });
    let _cancellation = CaptureCancellation {
        flag,
        worker: worker.abort_handle(),
    };
    worker.await.context("workspace capture worker failed")?
}
async fn capture_workspace(root: &Path) -> Result<Snapshot> {
    Ok(capture_background(root, false).await?.0)
}
async fn write_chunks(file: &mut File, bytes: &[u8], started: Instant) -> Result<()> {
    for chunk in bytes.chunks(64 * 1024) {
        tokio::task::yield_now().await;
        ensure!(
            started.elapsed() < DEADLINE,
            "workspace write exceeded 60 seconds"
        );
        file.write_all(chunk)?;
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationPlan {
    pub child_digest: String,
    pub result_commit: String,
    pub changed_paths: Vec<String>,
    patch_digest: String,
}

async fn limited(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    reader
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut out)
        .await?;
    ensure!(out.len() <= LIMIT, "Git output exceeds integration limit");
    Ok(out)
}

/// Plumbing commands consume raw bytes, never checkout/add content filters.
/// Configuration cannot enable hooks, signing, external diff, or fsmonitor.
async fn git(root: &Path, index: Option<&Path>, args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    let mut command = Command::new("/usr/bin/git");
    command
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "DemonCoder")
        .env("GIT_AUTHOR_EMAIL", "demoncoder@localhost")
        .env("GIT_COMMITTER_NAME", "DemonCoder")
        .env("GIT_COMMITTER_EMAIL", "demoncoder@localhost")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.safecrlf=false",
            "-c",
            "core.fileMode=true",
            "-c",
            "core.symlinks=true",
            "-c",
            "diff.external=",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "worktree.useRelativePaths=false",
        ])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command
        .spawn()
        .context("starting parent-owned Git command")?;
    let mut group = GitGroup(
        child
            .id()
            .and_then(|pid| rustix::process::Pid::from_raw(pid as i32)),
    );
    let mut stdin = child.stdin.take().context("Git stdin missing")?;
    let stdout = child.stdout.take().context("Git stdout missing")?;
    let stderr = child.stderr.take().context("Git stderr missing")?;
    let result = tokio::time::timeout(DEADLINE, async {
        let (out, err, status, ()) = tokio::try_join!(
            limited(stdout),
            limited(stderr),
            async { Ok::<_, anyhow::Error>(child.wait().await?) },
            async {
                stdin.write_all(input).await?;
                drop(stdin);
                Ok::<_, anyhow::Error>(())
            }
        )?;
        ensure!(
            status.success(),
            "Git {:?} failed ({status}): {} {}",
            args,
            String::from_utf8_lossy(&err),
            String::from_utf8_lossy(&out)
        );
        Ok(out)
    })
    .await
    .context("parent-owned Git command exceeded 60 seconds; inspect retained work before retrying")
    .and_then(|result| result);
    if result.is_err() {
        group.stop()?;
        tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .context("Git did not stop within two seconds")??;
    } else {
        group.0 = None;
    }
    result
}

fn line(bytes: Vec<u8>) -> Result<String> {
    Ok(String::from_utf8(bytes)
        .context("Git returned non-UTF-8 identity")?
        .trim_end_matches('\n')
        .to_owned())
}
async fn value(root: &Path, args: &[&str]) -> Result<String> {
    line(git(root, None, args, &[]).await?)
}
fn content_name(path: &str) -> &str {
    path.strip_prefix("./").unwrap_or(path)
}
fn valid_oid(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn path_text(path: &Path) -> Result<&str> {
    path.to_str().context("worktree path must be UTF-8")
}
fn content_equal(left: &Snapshot, right: &Snapshot) -> bool {
    left.entries
        .iter()
        .filter(|(p, _)| p.as_str() != ".")
        .eq(right.entries.iter().filter(|(p, _)| p.as_str() != "."))
}
fn changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    before
        .entries
        .keys()
        .chain(after.entries.keys())
        .filter(|p| p.as_str() != ".")
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|p| before.entries.get(*p) != after.entries.get(*p))
        .cloned()
        .collect()
}
fn secure_root(root: &Path) -> Result<File> {
    Ok(openat2(
        rustix::fs::CWD,
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?
    .into())
}

async fn materialize(
    root: &Path,
    snapshot: &Snapshot,
    raw: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let root = secure_root(root)?;
    let started = Instant::now();
    for (name, entry) in &snapshot.entries {
        tokio::task::yield_now().await;
        if name == "." {
            continue;
        }
        ensure!(
            started.elapsed() < DEADLINE,
            "baseline copy exceeded 60 seconds"
        );
        ensure!(
            valid_content_path(content_name(name)),
            "unsafe baseline path {name:?}"
        );
        let path = Path::new(name);
        let parent: File = openat2(
            &root,
            path.parent().unwrap_or(Path::new(".")),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
        )?
        .into();
        let leaf = path.file_name().context("missing baseline filename")?;
        match entry.kind {
            Kind::Directory => rustix::fs::mkdirat(&parent, leaf, Mode::from_raw_mode(0o700))?,
            Kind::Symlink => {
                let target =
                    std::ffi::OsStr::from_bytes(raw.get(name).context("missing symlink bytes")?);
                rustix::fs::symlinkat(target, &parent, leaf)?;
            }
            Kind::File => {
                let mut file: File = openat2(
                    &parent,
                    leaf,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::CLOEXEC
                        | OFlags::NOFOLLOW,
                    Mode::from_raw_mode(0o600),
                    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
                )?
                .into();
                write_chunks(
                    &mut file,
                    raw.get(name).context("missing baseline bytes")?,
                    started,
                )
                .await?;
                file.set_permissions(std::fs::Permissions::from_mode(entry.mode & 0o777))?;
            }
        }
    }
    for (name, entry) in snapshot.entries.iter().rev() {
        tokio::task::yield_now().await;
        ensure!(
            started.elapsed() < DEADLINE,
            "baseline permissions exceeded 60 seconds"
        );
        if name != "." && entry.kind == Kind::Directory {
            let directory: File = openat2(
                &root,
                Path::new(name),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
            )?
            .into();
            directory.set_permissions(std::fs::Permissions::from_mode(entry.mode & 0o777))?;
        }
    }
    Ok(())
}
use std::os::unix::ffi::OsStrExt;

async fn snapshot_tree(
    root: &Path,
    snapshot: &Snapshot,
    raw: &BTreeMap<String, Vec<u8>>,
) -> Result<String> {
    let temp = tempfile::tempdir()?;
    let index = temp.path().join("index");
    git(root, Some(&index), &["read-tree", "--empty"], &[]).await?;
    let mut paths = Vec::new();
    let mut entries = Vec::new();
    let started = Instant::now();
    for (name, entry) in &snapshot.entries {
        tokio::task::yield_now().await;
        if entry.kind == Kind::Directory {
            continue;
        }
        ensure!(
            valid_content_path(content_name(name)),
            "unsafe snapshot path {name:?}"
        );
        ensure!(
            started.elapsed() < DEADLINE,
            "snapshot export exceeded 60 seconds"
        );
        // Hash private, ordinary files in one plumbing invocation. The generated
        // numeric filenames cannot become options, quoted paths or line breaks.
        let blob = temp.path().join(format!("blob-{}", entries.len()));
        let mut file = File::create(&blob)?;
        write_chunks(
            &mut file,
            raw.get(name).context("missing captured bytes")?,
            started,
        )
        .await?;
        paths.extend_from_slice(path_text(&blob)?.as_bytes());
        paths.push(b'\n');
        entries.push((name, entry));
    }
    let hashes = git(
        root,
        None,
        &["hash-object", "-w", "--no-filters", "--stdin-paths"],
        &paths,
    )
    .await?;
    let hashes = std::str::from_utf8(&hashes)?.lines().collect::<Vec<_>>();
    ensure!(
        hashes.len() == entries.len(),
        "Git returned incomplete raw object hashes"
    );
    let mut records = Vec::new();
    for ((name, entry), oid) in entries.into_iter().zip(hashes) {
        tokio::task::yield_now().await;
        ensure!(
            (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|b| b.is_ascii_hexdigit()),
            "Git returned malformed object hash"
        );
        let mode = if entry.kind == Kind::Symlink {
            "120000"
        } else if entry.mode & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        };
        records.extend_from_slice(format!("{mode} {oid}\t{}\0", content_name(name)).as_bytes());
    }
    git(
        root,
        Some(&index),
        &["update-index", "-z", "--index-info"],
        &records,
    )
    .await?;
    line(git(root, Some(&index), &["write-tree"], &[]).await?)
}

async fn commit_snapshot(
    root: &Path,
    snapshot: &Snapshot,
    raw: &BTreeMap<String, Vec<u8>>,
    parent: &str,
) -> Result<String> {
    let tree = snapshot_tree(root, snapshot, raw).await?;
    value(
        root,
        &[
            "commit-tree",
            &tree,
            "-p",
            parent,
            "-m",
            "DemonCoder retained assignment snapshot",
        ],
    )
    .await
}

/// Create a genuine detached worktree containing the complete developer baseline.
/// The destination must not exist, and must be outside the selected parent tree.
pub async fn prepare(parent: &Path, destination: &Path) -> Result<WorktreeIdentity> {
    let (parent_baseline, raw) = capture_background(parent, true).await?;
    for (name, entry) in &parent_baseline.entries {
        tokio::task::yield_now().await;
        ensure!(
            entry.mode & 0o7000 == 0,
            "baseline has unsupported special permission bits: {name}"
        );
        if name != "." {
            ensure!(
                valid_content_path(content_name(name)),
                "unsafe baseline path {name:?}"
            );
        }
    }
    let parent = std::fs::canonicalize(parent)?;
    let destination_parent = destination
        .parent()
        .context("worktree destination has no parent")?;
    secure_root(destination_parent)?;
    let destination = std::fs::canonicalize(destination_parent)?.join(
        destination
            .file_name()
            .context("worktree destination has no filename")?,
    );
    ensure!(
        !destination.starts_with(&parent) && !parent.starts_with(&destination),
        "child worktree must be outside the parent workspace"
    );
    ensure!(
        !destination.try_exists()?,
        "worktree destination already exists"
    );
    ensure!(
        Path::new(&value(&parent, &["rev-parse", "--show-toplevel"]).await?) == parent,
        "selected workspace must be the Git worktree root"
    );
    let repository_head = value(&parent, &["rev-parse", "--verify", "HEAD^{commit}"]).await?;
    let common_dir = std::fs::canonicalize(
        value(
            &parent,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await?,
    )?;
    ensure!(
        !destination.starts_with(&common_dir),
        "child destination cannot be inside Git administration"
    );
    git(
        &parent,
        None,
        &[
            "worktree",
            "add",
            "--no-checkout",
            "--detach",
            path_text(&destination)?,
            &repository_head,
        ],
        &[],
    )
    .await?;
    materialize(&destination, &parent_baseline, &raw).await?;
    let child_baseline = capture_workspace(&destination).await?;
    ensure!(
        content_equal(&parent_baseline, &child_baseline),
        "copied baseline differs from the developer workspace"
    );
    let baseline_commit =
        commit_snapshot(&destination, &child_baseline, &raw, &repository_head).await?;
    git(
        &destination,
        None,
        &["update-ref", "HEAD", &baseline_commit],
        &[],
    )
    .await?;
    git(&destination, None, &["read-tree", &baseline_commit], &[]).await?;
    let git_dir =
        std::fs::canonicalize(value(&destination, &["rev-parse", "--absolute-git-dir"]).await?)?;
    ensure!(
        capture_workspace(&parent).await? == parent_baseline,
        "parent changed while preparing assignment; retain worktree and retry from a fresh baseline"
    );
    let git_meta = secure_root(&git_dir)?.metadata()?;
    let common_meta = secure_root(&common_dir)?.metadata()?;
    let identity = WorktreeIdentity {
        git_device: git_meta.dev(),
        git_inode: git_meta.ino(),
        common_device: common_meta.dev(),
        common_inode: common_meta.ino(),
        root: destination,
        git_dir,
        common_dir,
        repository_head,
        baseline_commit,
        parent_baseline,
        child_baseline,
    };
    inspect(&identity).await?;
    Ok(identity)
}

fn read_pointer(root: &Path, name: &str) -> Result<String> {
    let root = secure_root(root)?;
    let handle: File = openat2(
        &root,
        name,
        OFlags::PATH | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
    )?
    .into();
    let meta = handle.metadata()?;
    ensure!(
        meta.is_file() && meta.nlink() == 1 && meta.len() <= 8192,
        "Git pointer is not an independent bounded regular file"
    );
    let mut pointer = String::new();
    // Reopen the pinned regular inode, never a second untrusted pathname.
    File::open(format!("/proc/self/fd/{}", handle.as_raw_fd()))?
        .take(8193)
        .read_to_string(&mut pointer)?;
    let after = handle.metadata()?;
    ensure!(
        pointer.len() <= 8192
            && after.len() == meta.len()
            && (
                after.mtime(),
                after.mtime_nsec(),
                after.ctime(),
                after.ctime_nsec()
            ) == (
                meta.mtime(),
                meta.mtime_nsec(),
                meta.ctime(),
                meta.ctime_nsec()
            ),
        "Git pointer changed while reading"
    );
    Ok(pointer)
}

/// Verify administrative identity as well as all content before trusting a child.
pub async fn inspect(identity: &WorktreeIdentity) -> Result<Snapshot> {
    ensure!(
        valid_oid(&identity.baseline_commit) && valid_oid(&identity.repository_head),
        "invalid recorded Git commit identity"
    );
    let git_meta = secure_root(&identity.git_dir)?.metadata()?;
    let common_meta = secure_root(&identity.common_dir)?.metadata()?;
    ensure!(
        (git_meta.dev(), git_meta.ino()) == (identity.git_device, identity.git_inode),
        "child Git directory replaced"
    );
    ensure!(
        (common_meta.dev(), common_meta.ino()) == (identity.common_device, identity.common_inode),
        "shared Git directory replaced"
    );
    ensure!(
        identity.git_dir.parent() == Some(identity.common_dir.join("worktrees").as_path()),
        "child administrative directory must belong to the shared repository"
    );
    let pointer = identity.root.join(".git");
    ensure!(
        read_pointer(&identity.root, ".git")?
            == format!("gitdir: {}\n", identity.git_dir.display()),
        "child .git pointer changed"
    );
    ensure!(
        read_pointer(&identity.git_dir, "gitdir")? == format!("{}\n", pointer.display()),
        "child administrative backpointer changed"
    );
    let snapshot = capture_workspace(&identity.root).await?;
    ensure!(
        snapshot.same_root(&identity.child_baseline),
        "child workspace root replaced"
    );
    ensure!(
        Path::new(&value(&identity.root, &["rev-parse", "--show-toplevel"]).await?)
            == identity.root,
        "child worktree root changed"
    );
    ensure!(
        std::fs::canonicalize(value(&identity.root, &["rev-parse", "--absolute-git-dir"]).await?)?
            == identity.git_dir,
        "child Git administrative identity changed"
    );
    ensure!(
        std::fs::canonicalize(
            value(
                &identity.root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"]
            )
            .await?
        )? == identity.common_dir,
        "shared Git administrative identity changed"
    );
    ensure!(
        value(&identity.root, &["rev-parse", "HEAD"]).await? == identity.baseline_commit,
        "child Git HEAD changed outside parent control"
    );
    Ok(snapshot)
}

pub async fn build_delta(
    identity: &WorktreeIdentity,
    request: &AssignmentRequest,
    validated_digest: &str,
) -> Result<IntegrationPlan> {
    request.validate()?;
    let current = inspect(identity).await?;
    ensure!(
        current.digest == validated_digest,
        "child changed after validation"
    );
    ensure!(
        current.entries.get(".") == identity.child_baseline.entries.get("."),
        "child root directory permissions changed"
    );
    let changed_paths = changed(&identity.child_baseline, &current);
    for path in &changed_paths {
        tokio::task::yield_now().await;
        if let Some(entry) = current.entries.get(path) {
            ensure!(
                entry.mode & 0o7000 == 0,
                "child has unsupported special permission bits: {path}"
            );
        }
        if request.owns(content_name(path)) {
            continue;
        }
        // Directories created to hold owned files are allowed only when all
        // changed descendants are owned; other changes remain explicit.
        let directory = current
            .entries
            .get(path)
            .or(identity.child_baseline.entries.get(path))
            .is_some_and(|e| e.kind == Kind::Directory);
        let owned_directory = directory
            && (!identity.child_baseline.entries.contains_key(path)
                || !current.entries.contains_key(path))
            && changed_paths.iter().any(|p| {
                p != path && Path::new(p).starts_with(path) && request.owns(content_name(p))
            });
        ensure!(
            request.owns(content_name(path)) || owned_directory,
            "child changed unowned path {path:?}"
        );
    }
    let (captured, raw) = capture_background(&identity.root, true).await?;
    ensure!(
        captured == current,
        "child changed while preparing integration"
    );
    let result_commit =
        commit_snapshot(&identity.root, &current, &raw, &identity.baseline_commit).await?;
    // A named ref retains each validated result even if the worktree is removed.
    git(
        &identity.root,
        None,
        &[
            "update-ref",
            &format!("refs/demoncoder/results/{result_commit}"),
            &result_commit,
        ],
        &[],
    )
    .await?;
    let patch = git(
        &identity.root,
        None,
        &[
            "diff",
            "--binary",
            "--full-index",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            &identity.baseline_commit,
            &result_commit,
            "--",
        ],
        &[],
    )
    .await?;
    ensure!(
        inspect(identity).await? == current,
        "child changed while generating delta"
    );
    Ok(IntegrationPlan {
        child_digest: current.digest,
        result_commit,
        changed_paths,
        patch_digest: format!("{:x}", Sha256::digest(&patch)),
    })
}

/// Apply only the exact child delta. The parent index is never read or written.
/// Conflicts and stale validation fail before effects. External writers must
/// remain stopped during application; callers persist intent for interruption.
pub async fn integrate(
    parent: &Path,
    identity: &WorktreeIdentity,
    plan: &IntegrationPlan,
) -> Result<Snapshot> {
    ensure!(
        valid_oid(&plan.result_commit),
        "invalid integration result commit identity"
    );
    let child = inspect(identity).await?;
    ensure!(
        child.digest == plan.child_digest,
        "child changed after validation"
    );
    ensure!(
        changed(&identity.child_baseline, &child) == plan.changed_paths,
        "integration plan paths do not match child delta"
    );
    let (captured, raw) = capture_background(&identity.root, true).await?;
    ensure!(
        captured == child,
        "child changed before result verification"
    );
    ensure!(
        snapshot_tree(&identity.root, &child, &raw).await?
            == value(
                &identity.root,
                &["rev-parse", &format!("{}^{{tree}}", plan.result_commit)]
            )
            .await?,
        "retained result does not match validated child contents"
    );
    let actual_patch = git(
        &identity.root,
        None,
        &[
            "diff",
            "--binary",
            "--full-index",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            &identity.baseline_commit,
            &plan.result_commit,
            "--",
        ],
        &[],
    )
    .await?;
    ensure!(
        format!("{:x}", Sha256::digest(&actual_patch)) == plan.patch_digest,
        "integration plan patch changed"
    );
    let current = capture_workspace(parent).await?;
    ensure!(
        current.same_root(&identity.parent_baseline),
        "parent workspace identity changed"
    );
    ensure!(
        std::fs::canonicalize(
            value(
                parent,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"]
            )
            .await?
        )? == identity.common_dir,
        "parent repository identity changed"
    );
    ensure!(
        value(parent, &["rev-parse", "HEAD"]).await? == identity.repository_head,
        "parent Git HEAD changed since assignment"
    );
    let mut expected = current.clone();
    for path in &plan.changed_paths {
        tokio::task::yield_now().await;
        ensure!(
            current.entries.get(path) == identity.parent_baseline.entries.get(path),
            "parent conflict at {path:?}; developer content changed since assignment"
        );
        if let Some(entry) = child.entries.get(path) {
            expected.entries.insert(path.clone(), entry.clone());
        } else {
            expected.entries.remove(path);
        }
    }
    // Removing a directory cannot silently remove a later developer addition.
    for name in expected.entries.keys().filter(|name| name.as_str() != ".") {
        tokio::task::yield_now().await;
        let ancestor = Path::new(name)
            .parent()
            .and_then(Path::to_str)
            .context("invalid expected path")?;
        ensure!(
            expected
                .entries
                .get(ancestor)
                .is_some_and(|entry| entry.kind == Kind::Directory),
            "parent conflict: {name:?} would lose its containing directory"
        );
    }
    if !actual_patch.is_empty() {
        // Private repository configuration prevents filter execution. Its
        // highest-priority info/attributes explicitly disables byte transforms,
        // including those requested by any developer .gitattributes file.
        let application = tempfile::tempdir()?;
        git(
            application.path(),
            None,
            &["init", "--bare", "--template="],
            &[],
        )
        .await?;
        std::fs::create_dir_all(application.path().join("info"))?;
        std::fs::write(
            application.path().join("info/attributes"),
            b"* -text -filter -ident !working-tree-encoding !eol\n",
        )?;
        let git_dir_arg = format!("--git-dir={}", application.path().display());
        let work_tree_arg = format!("--work-tree={}", parent.display());
        git(
            parent,
            None,
            &[
                &git_dir_arg,
                &work_tree_arg,
                "apply",
                "--check",
                "--whitespace=nowarn",
                "-",
            ],
            &actual_patch,
        )
        .await?;
        ensure!(
            capture_workspace(parent).await? == current && inspect(identity).await? == child,
            "workspace changed before integration"
        );
        enable_validated_directory_writes(parent, &current, &expected, &plan.changed_paths).await?;
        git(
            parent,
            None,
            &[
                &git_dir_arg,
                &work_tree_arg,
                "apply",
                "--whitespace=nowarn",
                "-",
            ],
            &actual_patch,
        )
        .await?;
    }
    if actual_patch.is_empty() {
        ensure!(
            capture_workspace(parent).await? == current && inspect(identity).await? == child,
            "workspace changed before permission integration"
        );
    }
    reconcile_metadata(parent, &current, &expected, &plan.changed_paths).await?;
    let result = capture_workspace(parent).await?;
    ensure!(
        result.entries == expected.entries,
        "integration result differs at {:?}; inspect parent state before retrying",
        changed(&expected, &result)
    );
    Ok(result)
}

/// An explicitly validated directory permission increase must precede writes
/// beneath it. Restrictive changes remain deferred until after patch application.
async fn enable_validated_directory_writes(
    root: &Path,
    before: &Snapshot,
    after: &Snapshot,
    changes: &[String],
) -> Result<()> {
    let root = secure_root(root)?;
    let started = Instant::now();
    for name in changes {
        tokio::task::yield_now().await;
        ensure!(
            started.elapsed() < DEADLINE,
            "directory permission integration exceeded 60 seconds"
        );
        let (Some(old), Some(new)) = (before.entries.get(name), after.entries.get(name)) else {
            continue;
        };
        if old.kind != Kind::Directory
            || new.kind != Kind::Directory
            || old.mode & 0o200 != 0
            || new.mode & 0o200 == 0
        {
            continue;
        }
        let directory: File = openat2(
            &root,
            Path::new(name),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
        )?
        .into();
        directory.set_permissions(std::fs::Permissions::from_mode(new.mode & 0o777))?;
    }
    Ok(())
}

/// Git omits empty directories and normalizes permission bits. These narrowly
/// scoped operations restore the validated filesystem metadata after its patch.
async fn reconcile_metadata(
    root: &Path,
    before: &Snapshot,
    after: &Snapshot,
    changes: &[String],
) -> Result<()> {
    let root = secure_root(root)?;
    let started = Instant::now();
    let resolve = ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV;
    for name in changes.iter().rev() {
        tokio::task::yield_now().await;
        ensure!(
            started.elapsed() < DEADLINE,
            "metadata integration exceeded 60 seconds"
        );
        if before
            .entries
            .get(name)
            .is_some_and(|entry| entry.kind == Kind::Directory)
            && !after.entries.contains_key(name)
        {
            let path = Path::new(name);
            let parent = openat2(
                &root,
                path.parent().context("directory has no parent")?,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                resolve,
            );
            match parent {
                Err(rustix::io::Errno::NOENT) => {}
                Err(error) => return Err(error).context("opening removed directory parent"),
                Ok(parent) => match rustix::fs::unlinkat(
                    &parent,
                    path.file_name().context("directory has no name")?,
                    rustix::fs::AtFlags::REMOVEDIR,
                ) {
                    Ok(()) | Err(rustix::io::Errno::NOENT) => {}
                    Err(error) => return Err(error).context("removing validated empty directory"),
                },
            }
        }
    }
    let mut modes: BTreeSet<String> = changes.iter().cloned().collect();
    for (name, entry) in &after.entries {
        tokio::task::yield_now().await;
        ensure!(
            started.elapsed() < DEADLINE,
            "metadata integration exceeded 60 seconds"
        );
        if name == "." || entry.kind != Kind::Directory {
            continue;
        }
        match openat2(
            &root,
            Path::new(name),
            OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            resolve,
        ) {
            Ok(_) => {}
            Err(rustix::io::Errno::NOENT) => {
                let path = Path::new(name);
                let parent = openat2(
                    &root,
                    path.parent().context("directory has no parent")?,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                    Mode::empty(),
                    resolve,
                )?;
                rustix::fs::mkdirat(
                    &parent,
                    path.file_name().context("directory has no name")?,
                    Mode::from_raw_mode(0o700),
                )?;
                modes.insert(name.clone());
            }
            Err(error) => return Err(error).context("checking validated directory"),
        }
    }
    // Restrictive directory permissions are applied after their descendants.
    for name in modes.iter().rev() {
        tokio::task::yield_now().await;
        ensure!(
            started.elapsed() < DEADLINE,
            "metadata integration exceeded 60 seconds"
        );
        let Some(entry) = after.entries.get(name) else {
            continue;
        };
        if entry.kind == Kind::Symlink {
            continue;
        }
        let file: File = openat2(
            &root,
            Path::new(name),
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
            resolve,
        )?
        .into();
        let meta = file.metadata()?;
        ensure!(
            (entry.kind == Kind::Directory && meta.is_dir())
                || (entry.kind == Kind::File && meta.is_file() && meta.nlink() == 1),
            "integration target type changed: {name}"
        );
        file.set_permissions(std::fs::Permissions::from_mode(entry.mode & 0o777))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn materialization_yields_to_timers_and_stops_writes_when_cancelled() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        for index in 0..7 {
            std::fs::write(
                source.path().join(format!("file-{index}")),
                vec![b'x'; 8 * 1024 * 1024],
            )
            .unwrap();
        }
        let (snapshot, raw) = capture_background(source.path(), true).await.unwrap();
        let target = destination.path().to_owned();
        let task = tokio::spawn(async move { materialize(&target, &snapshot, &raw).await });
        let first = destination.path().join("file-0");
        let mut ticks = tokio::time::interval(Duration::from_millis(1));
        ticks.tick().await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                ticks.tick().await;
                if first.metadata().is_ok_and(|meta| meta.len() > 0) {
                    break;
                }
            }
        })
        .await
        .expect("materialization blocked the timer");
        assert!(
            !task.is_finished(),
            "fixture must cancel a partial multi-chunk copy"
        );
        let cancelled = Instant::now();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(cancelled.elapsed() < Duration::from_secs(2));
        let stopped = capture_workspace(destination.path()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            capture_workspace(destination.path()).await.unwrap(),
            stopped,
            "cancelled materialization kept changing destination files"
        );
    }

    #[tokio::test]
    async fn metadata_reconciliation_stops_at_cancellation_without_detached_mutations() {
        let destination = tempfile::tempdir().unwrap();
        for index in 0..500 {
            let directory = destination.path().join(format!("dir-{index:04}"));
            std::fs::create_dir(&directory).unwrap();
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let before = capture_workspace(destination.path()).await.unwrap();
        let mut after = before.clone();
        let mut changes = Vec::new();
        for (name, entry) in &mut after.entries {
            if name == "." {
                continue;
            }
            entry.mode = (entry.mode & !0o777) | 0o750;
            changes.push(name.clone());
        }
        let root = destination.path().to_owned();
        let task =
            tokio::spawn(async move { reconcile_metadata(&root, &before, &after, &changes).await });
        let first = destination.path().join("dir-0499");
        tokio::time::timeout(Duration::from_secs(1), async {
            while first.metadata().unwrap().permissions().mode() & 0o777 != 0o750 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let stopped = capture_workspace(destination.path()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            capture_workspace(destination.path()).await.unwrap(),
            stopped
        );
    }

    #[tokio::test]
    async fn cancelling_git_stops_its_owned_descendants() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_owned();
        let started = root.join("started");
        let escaped = root.join("escaped");
        // Test-only alias gives the plumbing runner a real child/grandchild
        // lifetime to revoke. Production never accepts alias names or scripts.
        let alias = format!(
            "alias.wait=!sh -c '(sleep 0.4; touch {}) & touch {}; wait'",
            escaped.display(),
            started.display()
        );
        let task =
            tokio::spawn(async move { git(&root, None, &["-c", &alias, "wait"], &[]).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !started.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(!escaped.exists(), "Git descendant survived cancellation");
    }
}
