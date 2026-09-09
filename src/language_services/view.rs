//! Admission happens before content reads. Children only see independent copied inodes.
use super::LanguageServers;
use anyhow::{Context, Result, ensure};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rustix::fs::{Dir, Mode, OFlags, ResolveFlags, openat2};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tokio::process::Command;

const SYSTEM: &[&str] = &[
    "/usr/bin",
    "/usr/lib",
    "/usr/lib64",
    "/bin",
    "/lib",
    "/lib64",
    "/etc/ld.so.cache",
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/nsswitch.conf",
    "/etc/ssl/certs/ca-certificates.crt",
];
const RESOLVE: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_XDEV);
const MAX_DENIALS: usize = 4096;
const MAX_ENTRIES: usize = 200_000;
const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const PROJECT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
struct Denial {
    reason: &'static str,
    version: u8,
}
#[derive(Default)]
pub(super) struct Denials(BTreeMap<PathBuf, Denial>, Option<[u8; 32]>);
impl Denials {
    fn policy(&mut self, private: &[PathBuf]) {
        let mut digest = Sha256::new();
        digest.update([crate::export_policy::VERSION]);
        for root in private {
            digest.update(root.as_os_str().as_bytes());
            digest.update([0]);
        }
        let version = digest.finalize().into();
        if self.1 != Some(version) {
            self.0.clear();
            self.1 = Some(version);
        }
    }

    fn record(&mut self, path: &Path, reason: &'static str) {
        if self.0.len() >= MAX_DENIALS {
            self.0.pop_first();
        }
        self.0.insert(
            path.to_owned(),
            Denial {
                reason,
                version: crate::export_policy::VERSION,
            },
        );
    }
    pub(super) fn count(&self) -> usize {
        self.0
            .values()
            .filter(|entry| {
                entry.version == crate::export_policy::VERSION && !entry.reason.is_empty()
            })
            .count()
    }
}

pub(super) struct View {
    _storage: tempfile::TempDir,
    mounts: Vec<(PathBuf, PathBuf)>,
    pub(super) manifest: BTreeMap<PathBuf, [u8; 32]>,
    fingerprints: BTreeMap<PathBuf, (Stamp, PathBuf)>,
    modes: BTreeMap<PathBuf, u32>,
    rules: BTreeMap<PathBuf, Option<Stamp>>,
    aliases: BTreeMap<PathBuf, Stamp>,
    directories: std::sync::Mutex<BTreeMap<PathBuf, Stamp>>,
    config: LanguageServers,
    private: Vec<PathBuf>,
    explicit: Vec<PathBuf>,
    filter: File,
    path: String,
}

struct Scan<'a> {
    private: &'a [PathBuf],
    cancelled: &'a AtomicBool,
    started: Instant,
    runtime: bool,
    entries: usize,
    bytes: u64,
    denials: &'a mut Denials,
    manifest: BTreeMap<PathBuf, [u8; 32]>,
    explicit: &'a [PathBuf],
    previous: Option<&'a View>,
    fingerprints: BTreeMap<PathBuf, (Stamp, PathBuf)>,
    modes: BTreeMap<PathBuf, u32>,
    rules: BTreeMap<PathBuf, Option<Stamp>>,
    aliases: BTreeMap<PathBuf, Stamp>,
    directories: BTreeMap<PathBuf, Stamp>,
}
impl Scan<'_> {
    fn checkpoint(&self) -> Result<()> {
        ensure!(
            !self.cancelled.load(Ordering::Relaxed),
            "language view admission cancelled"
        );
        ensure!(
            self.started.elapsed() < Duration::from_secs(if self.runtime { 120 } else { 10 }),
            "language view admission exceeded its bounded deadline; select smaller read roots"
        );
        ensure!(
            self.entries <= MAX_ENTRIES,
            "language view exceeds 200000 entries; select smaller read roots"
        );
        Ok(())
    }
    fn denied(&mut self, path: &Path) -> bool {
        // Only stable private-policy decisions are reusable. Ignore decisions
        // are reconsidered against the current descriptor-read rule files.
        if self.denials.0.get(path).is_some_and(|entry| {
            entry.version == crate::export_policy::VERSION && entry.reason == "private policy"
        }) {
            return true;
        }
        // A miss or eviction never grants access: apply the rules before bytes.
        let reason = if crate::export_policy::private_path(path)
            || self.private.iter().any(|root| path.starts_with(root))
        {
            Some("private policy")
        } else if path.components().any(|part| part.as_os_str() == ".git") {
            Some("Git metadata")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.denials.record(path, reason);
            true
        } else {
            false
        }
    }
    fn open(root: &File, path: &Path, flags: OFlags) -> Result<File> {
        Ok(openat2(
            root,
            path,
            flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
            RESOLVE,
        )?
        .into())
    }
    fn copy_file(&mut self, handle: &File, output: &Path) -> Result<[u8; 32]> {
        self.checkpoint()?;
        let before = handle.metadata()?;
        ensure!(
            before.is_file() && before.nlink() == 1,
            "language admission refuses special files and hard links"
        );
        let limit = if self.runtime {
            MAX_BYTES
        } else {
            PROJECT_BYTES
        };
        self.bytes = self
            .bytes
            .checked_add(before.len())
            .context("language view byte counter overflow")?;
        ensure!(
            self.bytes <= limit,
            "language view exceeds {} MiB; select smaller roots or ignore generated files",
            limit / 1024 / 1024
        );
        let mut input = File::open(format!("/proc/self/fd/{}", handle.as_raw_fd()))?;
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .open(output)?;
        let cloned = match rustix::fs::ioctl_ficlone(&target, &input) {
            Ok(()) => true,
            Err(error)
                if [
                    rustix::io::Errno::OPNOTSUPP,
                    rustix::io::Errno::XDEV,
                    rustix::io::Errno::INVAL,
                    rustix::io::Errno::NOTTY,
                ]
                .contains(&error) =>
            {
                false
            }
            Err(error) => return Err(error).context("clone admitted language file"),
        };
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut chunk = [0u8; 64 * 1024];
        // Source revisions use content hashes. Runtime revisions use the pinned
        // inode's size and nanosecond modification/change stamps, also used for
        // reuse validation; hashing multi-gigabyte toolchains on every new
        // session would consume most of the startup allowance.
        if self.runtime {
            digest.update(format!("{:?}", stamp(&before)).as_bytes());
        }
        if cloned && self.runtime {
            count = before.len();
        }
        if !(cloned && self.runtime) {
            loop {
                self.checkpoint()?;
                let n = input.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                ensure!(
                    count <= before.len(),
                    "language source grew during admission; retry"
                );
                if !self.runtime {
                    digest.update(&chunk[..n]);
                }
                if !cloned {
                    target.write_all(&chunk[..n])?;
                }
            }
        }
        let after = input.metadata()?;
        ensure!(
            count == before.len()
                && target.metadata()?.len() == before.len()
                && stamp(&before) == stamp(&after),
            "language source changed during admission; retry"
        );
        target.set_permissions(std::fs::Permissions::from_mode(
            before.mode() & 0o555 | 0o400,
        ))?;
        Ok(digest.finalize().into())
    }
    fn walk(
        &mut self,
        root: &File,
        relative: &Path,
        original: &Path,
        output: &Path,
        rules: &[Gitignore],
        depth: usize,
    ) -> Result<()> {
        self.checkpoint()?;
        ensure!(depth <= 64, "language view exceeds 64 directory levels");
        self.entries += 1;
        if self.denied(original) {
            return Ok(());
        }
        let handle = match Self::open(root, relative, OFlags::PATH) {
            Ok(file) => file,
            Err(error) => {
                self.denials.record(original, "unsafe alias or mount");
                return Err(error)
                    .context("language admission refuses symlink traversal or mounted subtree");
            }
        };
        let meta = handle.metadata()?;
        if !self.runtime
            && !self.explicit.iter().any(|path| {
                path == original || path.starts_with(original) || original.starts_with(path)
            })
        {
            let ignored = rules
                .iter()
                .rev()
                .find_map(|rule| {
                    let matched = rule.matched(original, meta.is_dir());
                    if matched.is_ignore() {
                        Some(true)
                    } else if matched.is_whitelist() {
                        Some(false)
                    } else {
                        None
                    }
                })
                .unwrap_or(false);
            if ignored {
                self.denials.record(original, "Git ignore");
                return Ok(());
            }
        }
        self.denials.0.remove(original);
        if meta.is_dir() {
            self.directories.insert(original.to_owned(), stamp(&meta));
            std::fs::create_dir_all(output)?;
            let dir = Self::open(root, relative, OFlags::RDONLY | OFlags::DIRECTORY)?;
            ensure!(
                stamp(&meta) == stamp(&dir.metadata()?),
                "language directory changed during admission"
            );
            let mut nested = rules.to_vec();
            if !self.runtime {
                let ignore_path = relative.join(".gitignore");
                let ignore_identity = Self::open(root, &ignore_path, OFlags::PATH)
                    .ok()
                    .and_then(|file| file.metadata().ok())
                    .map(|metadata| stamp(&metadata));
                self.rules
                    .insert(original.join(".gitignore"), ignore_identity);
                // Only read a descriptor-classified regular ignore file. A
                // symlink or special ignore entry never controls admission.
                if let Ok(ignore) = Self::open(root, &ignore_path, OFlags::PATH) {
                    let metadata = ignore.metadata()?;
                    if metadata.is_file()
                        && metadata.nlink() == 1
                        && metadata.len() <= 1024 * 1024
                        && !self.denied(&original.join(".gitignore"))
                    {
                        let file = File::open(format!("/proc/self/fd/{}", ignore.as_raw_fd()))?;
                        let mut bytes = Vec::new();
                        file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
                        ensure!(
                            bytes.len() <= 1024 * 1024,
                            "language Git ignore file grew beyond limit"
                        );
                        let mut builder = GitignoreBuilder::new(original);
                        for line in std::str::from_utf8(&bytes)
                            .context("language Git ignore must be UTF-8")?
                            .lines()
                        {
                            builder.add_line(None, line)?;
                        }
                        nested.push(builder.build()?);
                    }
                }
            }
            for entry in Dir::read_from(&dir)? {
                self.checkpoint()?;
                let entry = entry?;
                let name = entry.file_name().to_bytes();
                if name == b"." || name == b".." {
                    continue;
                }
                let name =
                    std::str::from_utf8(name).context("language view paths must be UTF-8")?;
                self.walk(
                    root,
                    &relative.join(name),
                    &original.join(name),
                    &output.join(name),
                    &nested,
                    depth + 1,
                )?;
            }
        } else if meta.is_file() && meta.nlink() == 1 {
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let identity = stamp(&meta);
            let reusable = self.previous.and_then(|view| {
                view.fingerprints
                    .get(original)
                    .filter(|(old, _)| *old == identity)
                    .map(|(_, path)| (path, view.manifest[original]))
            });
            let digest = if let Some((previous, digest)) = reusable {
                self.bytes += meta.len();
                ensure!(
                    self.bytes
                        <= if self.runtime {
                            MAX_BYTES
                        } else {
                            PROJECT_BYTES
                        },
                    "language view exceeds its admitted byte limit"
                );
                // Both names belong to this owner, never to a host input. The
                // child has read-only mounts and cannot alter either inode.
                std::fs::hard_link(previous, output).context("reuse independent admitted copy")?;
                digest
            } else {
                self.copy_file(&handle, output)?
            };
            self.fingerprints
                .insert(original.to_owned(), (identity, output.to_owned()));
            self.manifest.insert(original.to_owned(), digest);
            self.modes.insert(original.to_owned(), meta.mode() & 0o777);
        } else if meta.is_symlink() && self.runtime {
            self.aliases.insert(original.to_owned(), stamp(&meta));
            // Resolve only the pinned link text. Restart beneath this selected
            // root; NO_SYMLINKS prevents a second alias from escaping authority.
            let target = rustix::fs::readlinkat(&handle, "", Vec::new())?;
            let target = Path::new(std::ffi::OsStr::from_bytes(target.to_bytes()));
            let base = original
                .ancestors()
                .nth(
                    relative
                        .components()
                        .filter(|part| matches!(part, std::path::Component::Normal(_)))
                        .count(),
                )
                .context("invalid runtime root")?;
            let lexical = if target.is_absolute() {
                target.to_owned()
            } else {
                original
                    .parent()
                    .context("runtime link has no parent")?
                    .join(target)
            };
            let normalized = normalize(&lexical)?;
            ensure!(
                normalized.starts_with(base),
                "runtime alias leaves its explicitly selected root; select the actual package root"
            );
            ensure!(
                !self.denied(&normalized),
                "runtime alias targets private data"
            );
            self.walk(
                root,
                normalized.strip_prefix(base)?,
                &normalized,
                output,
                rules,
                depth + 1,
            )?;
            // Preserve the alias name in the view identity as well as the
            // resolved target. Removing an alias must retire the old mount.
            let aliases: Vec<_> = self
                .manifest
                .iter()
                .filter_map(|(path, digest)| {
                    path.strip_prefix(&normalized)
                        .ok()
                        .map(|suffix| (original.join(suffix), *digest))
                })
                .collect();
            self.manifest.extend(aliases);
            let alias_modes: Vec<_> = self
                .modes
                .iter()
                .filter_map(|(path, mode)| {
                    path.strip_prefix(&normalized)
                        .ok()
                        .map(|suffix| (original.join(suffix), *mode))
                })
                .collect();
            self.modes.extend(alias_modes);
        } else {
            self.denials
                .record(original, "alias, hard link or special file");
        }
        Ok(())
    }
}
use std::os::unix::ffi::OsStrExt;
type Stamp = (u64, u64, u64, i64, i64, i64, i64);
fn stamp(meta: &std::fs::Metadata) -> Stamp {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}
fn normalize(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                ensure!(result.pop(), "runtime alias escapes root");
            }
            std::path::Component::CurDir => {}
            part => result.push(part.as_os_str()),
        }
    }
    Ok(result)
}
pub(super) struct Source<'a> {
    pub(super) path: &'a Path,
    pub(super) root: &'a File,
}
impl View {
    pub(super) fn build(
        config: &LanguageServers,
        source: Source<'_>,
        private: &[PathBuf],
        explicit: &[PathBuf],
        denials: &mut Denials,
        cancelled: &AtomicBool,
        previous: Option<&View>,
    ) -> Result<Self> {
        let Source {
            path: workspace,
            root,
        } = source;
        denials.policy(private);
        ensure!(
            !private.iter().any(|private| SYSTEM.iter().any(|runtime| {
                let runtime = Path::new(runtime);
                private.starts_with(runtime)
                    || runtime.starts_with(private)
                    || runtime.canonicalize().is_ok_and(|physical| {
                        private.starts_with(&physical) || physical.starts_with(private)
                    })
            })),
            "private root overlaps the language system runtime"
        );
        ensure!(
            !private.iter().any(|private| workspace.starts_with(private)),
            "language workspace overlaps a private root"
        );
        ensure!(
            std::fs::read_link(format!("/proc/self/fd/{}", root.as_raw_fd()))? == workspace,
            "language workspace moved; reopen it"
        );
        let storage = tempfile::Builder::new()
            .prefix("demoncoder-language-")
            .tempdir()?;
        ensure!(
            !storage.path().starts_with(workspace),
            "language view storage must be outside the selected workspace"
        );
        let mut explicit = explicit.to_vec();
        explicit.extend(
            config
                .read_roots
                .iter()
                .filter(|path| path.starts_with(workspace))
                .cloned(),
        );
        explicit.extend(
            [&config.rust, &config.typescript]
                .into_iter()
                .flatten()
                .filter(|path| path.starts_with(workspace))
                .cloned(),
        );
        let mut scan = Scan {
            private,
            cancelled,
            started: Instant::now(),
            runtime: false,
            entries: 0,
            bytes: 0,
            denials,
            manifest: BTreeMap::new(),
            explicit: &explicit,
            previous,
            fingerprints: BTreeMap::new(),
            modes: BTreeMap::new(),
            rules: BTreeMap::new(),
            aliases: BTreeMap::new(),
            directories: BTreeMap::new(),
        };
        let project = storage.path().join("project");
        scan.walk(root, Path::new("."), workspace, &project, &[], 0)?;
        let mut mounts = vec![(project, workspace.to_owned())];
        let mut selected = config.read_roots.clone();
        for binary in [&config.rust, &config.typescript].into_iter().flatten() {
            if !binary.starts_with(workspace)
                && !SYSTEM.iter().any(|runtime| binary.starts_with(runtime))
                && !selected.iter().any(|root| binary.starts_with(root))
            {
                selected.push(binary.clone());
            }
        }
        selected.sort();
        selected.dedup();
        let mut roots: Vec<PathBuf> = Vec::new();
        for selected in selected {
            ensure!(
                selected.is_absolute() && normalize(&selected)? == selected,
                "language read roots must be normalized absolute paths"
            );
            if selected.starts_with(workspace)
                || roots.iter().any(|root| selected.starts_with(root))
            {
                continue;
            }
            ensure!(
                !workspace.starts_with(&selected),
                "language read root must not contain the workspace; select specific dependencies"
            );
            ensure!(
                !SYSTEM
                    .iter()
                    .any(|runtime| Path::new(runtime).starts_with(&selected)
                        || selected.starts_with(runtime)),
                "system language runtime is already available; select only external roots"
            );
            roots.push(selected);
        }
        scan.runtime = true;
        scan.started = Instant::now();
        scan.bytes = 0;
        let mut path = Vec::new();
        for (index, original) in roots.iter().enumerate() {
            ensure!(
                !scan.denied(original),
                "selected language runtime is private"
            );
            let host_root = File::open("/")?;
            let relative = original.strip_prefix("/")?;
            let handle: File = openat2(&host_root, relative, OFlags::PATH | OFlags::CLOEXEC | OFlags::NOFOLLOW, Mode::empty(), ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS).map(File::from).context("selected runtime has a symlink ancestor; pass the actual runtime path (for Rust, use rustup which rust-analyzer)")?;
            let output = storage.path().join(format!("runtime-{index}"));
            if handle.metadata()?.is_dir() {
                let directory = File::open(format!("/proc/self/fd/{}", handle.as_raw_fd()))?;
                scan.walk(&directory, Path::new("."), original, &output, &[], 0)?;
                path.push(original.to_string_lossy().into_owned());
                path.push(original.join("bin").to_string_lossy().into_owned());
            } else {
                ensure!(
                    handle.metadata()?.is_file() && handle.metadata()?.nlink() == 1,
                    "selected language executable must be an independent regular file; select its actual path"
                );
                let parent = original.parent().context("runtime file has no parent")?;
                let parent_fd: File = openat2(
                    &host_root,
                    parent.strip_prefix("/")?,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
                )?
                .into();
                scan.walk(
                    &parent_fd,
                    Path::new(original.file_name().context("runtime file has no name")?),
                    original,
                    &output,
                    &[],
                    0,
                )?;
                path.push(parent.to_string_lossy().into_owned());
            }
            mounts.push((output, original.clone()));
        }
        path.extend(["/usr/bin".into(), "/bin".into()]);
        let view = Self {
            _storage: storage,
            mounts,
            manifest: scan.manifest,
            fingerprints: scan.fingerprints,
            modes: scan.modes,
            rules: scan.rules,
            aliases: scan.aliases,
            directories: std::sync::Mutex::new(scan.directories),
            config: config.clone(),
            private: private.to_vec(),
            explicit,
            filter: crate::socket_filter::language_server_file()?,
            path: path.join(":"),
        };
        ensure!(
            view.metadata_current(workspace, root, cancelled, true)?,
            "language inputs changed during admission; retry"
        );
        Ok(view)
    }
    /// Validate the admitted inputs and ignore-policy files without rereading
    /// contents. Missing ignore files are recorded too, so a new policy cannot
    /// silently leave a cached peer's answer looking current.
    pub(super) fn inputs_current(
        &self,
        workspace: &Path,
        root: &File,
        cancelled: &AtomicBool,
    ) -> Result<bool> {
        if self.metadata_current(workspace, root, cancelled, true)? {
            return Ok(true);
        }
        if !self.metadata_current(workspace, root, cancelled, false)? {
            return Ok(false);
        }
        // A directory stamp is only a hint: private or ignored additions must
        // not retire a peer. Reuse the bounded admission policy to distinguish
        // them from newly visible dependencies, including empty selected roots.
        let candidate = Self::build(
            &self.config,
            Source {
                path: workspace,
                root,
            },
            &self.private,
            &self.explicit,
            &mut Denials::default(),
            cancelled,
            Some(self),
        )?;
        if !self.same_revision(&candidate) {
            return Ok(false);
        }
        *self
            .directories
            .lock()
            .map_err(|_| anyhow::anyhow!("language directory validation poisoned"))? = candidate
            .directories
            .into_inner()
            .map_err(|_| anyhow::anyhow!("language directory validation poisoned"))?;
        Ok(true)
    }

    fn metadata_current(
        &self,
        workspace: &Path,
        root: &File,
        cancelled: &AtomicBool,
        check_directories: bool,
    ) -> Result<bool> {
        let started = Instant::now();
        if std::fs::read_link(format!("/proc/self/fd/{}", root.as_raw_fd()))? != workspace {
            return Ok(false);
        }
        let host = File::open("/")?;
        let inspect = |path: &Path| -> Result<Option<std::fs::Metadata>> {
            ensure!(
                !cancelled.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(10),
                "language input validation exceeded its 10 second limit or was cancelled"
            );
            let file = if let Ok(relative) = path.strip_prefix(workspace) {
                openat2(
                    root,
                    if relative.as_os_str().is_empty() {
                        Path::new(".")
                    } else {
                        relative
                    },
                    OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                    RESOLVE,
                )
            } else {
                // Only metadata is checked here. Admission itself anchors each
                // explicitly selected root and forbids crossing mounts below it.
                openat2(
                    &host,
                    path.strip_prefix("/")?,
                    OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
                )
            };
            match file {
                Ok(file) => Ok(Some(File::from(file).metadata()?)),
                Err(
                    rustix::io::Errno::NOENT
                    | rustix::io::Errno::LOOP
                    | rustix::io::Errno::XDEV
                    | rustix::io::Errno::NOTDIR,
                ) => Ok(None),
                Err(error) => Err(error).context("validate admitted language input"),
            }
        };
        if check_directories {
            let directories = self
                .directories
                .lock()
                .map_err(|_| anyhow::anyhow!("language directory validation poisoned"))?;
            for (path, identity) in directories.iter() {
                if !inspect(path)?
                    .is_some_and(|metadata| metadata.is_dir() && stamp(&metadata) == *identity)
                {
                    return Ok(false);
                }
            }
        }
        for (path, (identity, _)) in &self.fingerprints {
            if !inspect(path)?.is_some_and(|metadata| {
                metadata.is_file() && metadata.nlink() == 1 && stamp(&metadata) == *identity
            }) {
                return Ok(false);
            }
        }
        for (path, identity) in &self.aliases {
            if !inspect(path)?
                .is_some_and(|metadata| metadata.is_symlink() && stamp(&metadata) == *identity)
            {
                return Ok(false);
            }
        }
        for (path, identity) in &self.rules {
            if inspect(path)?.map(|metadata| stamp(&metadata)) != *identity {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn same_revision(&self, other: &Self) -> bool {
        self.manifest == other.manifest
            && self.modes == other.modes
            && self.rules == other.rules
            && self.aliases == other.aliases
            && self
                .fingerprints
                .iter()
                .map(|(path, (identity, _))| (path, identity))
                .eq(other
                    .fingerprints
                    .iter()
                    .map(|(path, (identity, _))| (path, identity)))
    }

    pub(super) fn command(&self, workspace: &Path, script: &str) -> Command {
        let mut command = Command::new("/bin/bash");
        command.args(["--noprofile", "--norc", "-c", r#"exec 3<"$1" || exit
shift
for task_fd_path in /proc/self/fd/*; do
 task_fd=${task_fd_path##*/}
 case "$task_fd" in 0|1|2|3) ;; *) [[ "$task_fd" =~ ^[0-9]+$ ]] || exit 1; exec {task_fd}>&- || exit ;; esac
done
exec "$@""#, "demoncoder-language"])
            .arg(format!("/proc/{}/fd/{}", std::process::id(), self.filter.as_raw_fd()))
            .args(["/usr/bin/bwrap", "--seccomp", "3", "--unshare-all", "--share-net", "--die-with-parent", "--new-session"]);
        for path in SYSTEM {
            if Path::new(path).exists() {
                command.args(["--ro-bind", path, path]);
            }
        }
        command.args([
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--dir",
            "/tmp/home",
            "--dir",
            "/tmp/cargo",
            "--dir",
            "/tmp/target",
        ]);
        for (source, target) in &self.mounts {
            if target == workspace {
                // Cargo needs to write Cargo.lock. Only our admitted independent
                // copy is a lower layer; writes live in an invisible child-owned
                // tmpfs and disappear with that child. Admission never reads it.
                command
                    .arg("--overlay-src")
                    .arg(source)
                    .arg("--tmp-overlay")
                    .arg(target);
            } else {
                command.arg("--ro-bind").arg(source).arg(target);
            }
        }
        command
            .args([
                "--remount-ro",
                "/",
                "--remount-ro",
                "/proc",
                "--remount-ro",
                "/dev",
                "--clearenv",
                "--setenv",
                "PATH",
            ])
            .arg(&self.path)
            .args([
                "--setenv",
                "HOME",
                "/tmp/home",
                "--setenv",
                "CARGO_HOME",
                "/tmp/cargo",
                "--setenv",
                "CARGO_TARGET_DIR",
                "/tmp/target",
                "--setenv",
                "TMPDIR",
                "/tmp",
                "--setenv",
                "LANG",
                "C.UTF-8",
                "--chdir",
            ])
            .arg(workspace)
            .args(["/bin/bash", "--noprofile", "--norc", "-c", script])
            .env_clear();
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(workspace: &Path, config: &LanguageServers, denials: &mut Denials) -> Result<View> {
        View::build(
            config,
            Source {
                path: workspace,
                root: &File::open(workspace)?,
            },
            &[],
            &[],
            denials,
            &AtomicBool::new(false),
            None,
        )
    }

    #[test]
    fn copies_are_independent_and_ignore_changes_reconsider_denials() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join(".gitignore"), "generated/\n").unwrap();
        std::fs::create_dir(source.path().join("generated")).unwrap();
        std::fs::write(source.path().join("generated/public.rs"), "public").unwrap();
        std::fs::write(source.path().join("visible.rs"), "first").unwrap();
        std::fs::write(source.path().join(".env"), "synthetic-private").unwrap();
        let mut denials = Denials::default();
        let view = build(source.path(), &LanguageServers::default(), &mut denials).unwrap();
        let copied = view.mounts[0].0.join("visible.rs");
        assert_ne!(
            std::fs::metadata(&copied).unwrap().ino(),
            std::fs::metadata(source.path().join("visible.rs"))
                .unwrap()
                .ino()
        );
        std::fs::write(source.path().join("visible.rs"), "second").unwrap();
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "first");
        assert!(!view.mounts[0].0.join("generated/public.rs").exists());
        assert!(!view.mounts[0].0.join(".env").exists());
        assert!(denials.0.contains_key(&source.path().join("generated")));
        std::fs::write(source.path().join(".gitignore"), "").unwrap();
        let updated = build(source.path(), &LanguageServers::default(), &mut denials).unwrap();
        assert!(updated.mounts[0].0.join("generated/public.rs").is_file());
        assert!(!denials.0.contains_key(&source.path().join("generated")));
        assert!(denials.0.contains_key(&source.path().join(".env")));
        std::fs::write(source.path().join(".gitignore"), "visible.rs\n").unwrap();
        let removed = build(source.path(), &LanguageServers::default(), &mut denials).unwrap();
        assert!(
            !removed
                .manifest
                .contains_key(&source.path().join("visible.rs"))
        );
    }

    #[test]
    fn explicit_ignored_dependencies_still_exclude_private_data_and_aliases() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join(".gitignore"), "vendor/\n").unwrap();
        let vendor = source.path().join("vendor");
        std::fs::create_dir(&vendor).unwrap();
        std::fs::write(vendor.join("public.rs"), "public").unwrap();
        std::fs::write(vendor.join(".env"), "private").unwrap();
        std::os::unix::fs::symlink(".env", vendor.join("alias.rs")).unwrap();
        std::fs::hard_link(vendor.join(".env"), vendor.join("hard.rs")).unwrap();
        let config = LanguageServers {
            read_roots: vec![vendor],
            ..Default::default()
        };
        let view = build(source.path(), &config, &mut Denials::default()).unwrap();
        let copied = view.mounts[0].0.join("vendor");
        assert!(copied.join("public.rs").is_file());
        for name in [".env", "alias.rs", "hard.rs"] {
            assert!(!copied.join(name).exists(), "{name}");
        }
    }

    #[test]
    fn nested_gitignore_negations_do_not_escape_ignored_directory_boundaries() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join(".gitignore"), "*.tmp\nbuild/\n").unwrap();
        std::fs::create_dir(source.path().join("src")).unwrap();
        std::fs::write(source.path().join("src/.gitignore"), "!keep.tmp\n").unwrap();
        for name in ["src/keep.tmp", "src/drop.tmp"] {
            std::fs::write(source.path().join(name), "source").unwrap();
        }
        std::fs::create_dir(source.path().join("build")).unwrap();
        std::fs::write(source.path().join("build/.gitignore"), "!keep.tmp\n").unwrap();
        std::fs::write(source.path().join("build/keep.tmp"), "generated").unwrap();
        let view = build(
            source.path(),
            &LanguageServers::default(),
            &mut Denials::default(),
        )
        .unwrap();
        assert!(view.mounts[0].0.join("src/keep.tmp").exists());
        assert!(!view.mounts[0].0.join("src/drop.tmp").exists());
        assert!(!view.mounts[0].0.join("build").exists());
    }

    #[tokio::test]
    async fn project_writes_live_only_in_the_disposable_layer() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("source.rs"), "original").unwrap();
        let view = build(
            source.path(),
            &LanguageServers::default(),
            &mut Denials::default(),
        )
        .unwrap();
        let mut command = view.command(
            source.path(),
            "chmod u+w source.rs && printf changed > source.rs && printf generated > Cargo.lock",
        );
        let output = command.output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(source.path().join("source.rs")).unwrap(),
            "original"
        );
        assert_eq!(
            std::fs::read_to_string(view.mounts[0].0.join("source.rs")).unwrap(),
            "original"
        );
        assert!(!source.path().join("Cargo.lock").exists());
        assert!(!view.mounts[0].0.join("Cargo.lock").exists());
    }

    #[test]
    fn runtime_aliases_materialize_only_internal_public_targets() {
        let source = tempfile::tempdir().unwrap();
        let runtime = tempfile::tempdir().unwrap();
        std::fs::write(runtime.path().join("library.so.1"), "library").unwrap();
        std::os::unix::fs::symlink("library.so.1", runtime.path().join("library.so")).unwrap();
        let config = LanguageServers {
            read_roots: vec![runtime.path().to_owned()],
            ..Default::default()
        };
        let view = build(source.path(), &config, &mut Denials::default()).unwrap();
        let copied = view.mounts[1].0.join("library.so");
        assert!(copied.symlink_metadata().unwrap().is_file());
        assert_eq!(std::fs::read_to_string(copied).unwrap(), "library");
        assert!(
            view.manifest
                .contains_key(&runtime.path().join("library.so"))
        );
        std::os::unix::fs::symlink(source.path().join("outside"), runtime.path().join("escape"))
            .unwrap();
        assert!(build(source.path(), &config, &mut Denials::default()).is_err());
    }

    #[test]
    fn deny_cache_is_bounded_and_empty_cache_never_authorizes_private_content() {
        let mut denials = Denials::default();
        for index in 0..MAX_DENIALS + 100 {
            denials.record(&PathBuf::from(format!("private-{index}")), "private policy");
        }
        assert_eq!(denials.count(), MAX_DENIALS);
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("nested")).unwrap();
        std::fs::create_dir(source.path().join("nested/.demoncoder")).unwrap();
        std::fs::write(source.path().join("nested/.demoncoder/settings"), "private").unwrap();
        let view = build(
            source.path(),
            &LanguageServers::default(),
            &mut Denials::default(),
        )
        .unwrap();
        assert!(!view.mounts[0].0.join("nested/.demoncoder").exists());
    }

    #[test]
    fn input_validation_detects_new_policy_and_deletion_without_events() {
        let source = tempfile::tempdir().unwrap();
        let file = source.path().join("public.rs");
        std::fs::write(&file, "public").unwrap();
        let root = File::open(source.path()).unwrap();
        let cancelled = AtomicBool::new(false);
        let view = build(
            source.path(),
            &LanguageServers::default(),
            &mut Denials::default(),
        )
        .unwrap();
        assert!(
            view.inputs_current(source.path(), &root, &cancelled)
                .unwrap()
        );
        std::fs::write(source.path().join(".gitignore"), "public.rs\n").unwrap();
        assert!(
            !view
                .inputs_current(source.path(), &root, &cancelled)
                .unwrap()
        );
        std::fs::remove_file(source.path().join(".gitignore")).unwrap();
        assert!(
            view.inputs_current(source.path(), &root, &cancelled)
                .unwrap()
        );
        std::fs::remove_file(file).unwrap();
        assert!(
            !view
                .inputs_current(source.path(), &root, &cancelled)
                .unwrap()
        );
    }

    #[test]
    fn input_validation_detects_public_additions_without_events() {
        for external in [false, true] {
            let source = tempfile::tempdir().unwrap();
            let dependency = tempfile::tempdir().unwrap();
            std::fs::write(source.path().join(".gitignore"), "ignored*\n").unwrap();
            let config = LanguageServers {
                read_roots: if external {
                    vec![dependency.path().to_owned()]
                } else {
                    vec![]
                },
                ..LanguageServers::default()
            };
            let root = File::open(source.path()).unwrap();
            let cancelled = AtomicBool::new(false);
            let view = build(source.path(), &config, &mut Denials::default()).unwrap();
            std::fs::write(source.path().join(".env"), "private").unwrap();
            std::fs::write(source.path().join("ignored.rs"), "ignored").unwrap();
            assert!(
                view.inputs_current(source.path(), &root, &cancelled)
                    .unwrap()
            );
            let target = if external {
                dependency.path()
            } else {
                source.path()
            };
            std::fs::write(target.join("new_dependency.rs"), "public").unwrap();
            assert!(
                !view
                    .inputs_current(source.path(), &root, &cancelled)
                    .unwrap()
            );
        }
    }

    #[test]
    fn cancellation_and_system_private_overlap_fail_closed() {
        let source = tempfile::tempdir().unwrap();
        let config = LanguageServers::default();
        let root = File::open(source.path()).unwrap();
        assert!(
            View::build(
                &config,
                Source {
                    path: source.path(),
                    root: &root
                },
                &[],
                &[],
                &mut Denials::default(),
                &AtomicBool::new(true),
                None
            )
            .is_err()
        );
        assert!(
            View::build(
                &config,
                Source {
                    path: source.path(),
                    root: &root
                },
                &[PathBuf::from("/usr/lib/private")],
                &[],
                &mut Denials::default(),
                &AtomicBool::new(false),
                None
            )
            .is_err()
        );
    }
}
