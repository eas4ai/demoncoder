//! Private, crash-safe workflow records. Paths are resolved once; later I/O uses pinned descriptors.
//!
//! Failures before replacement preserve the previous record. If syncing the directory
//! fails after replacement, the new record is valid and visible but its survival across
//! a crash is uncertain. The caller must hold execution on any write error; an error
//! does not imply the old record remains installed. No rollback is attempted.
use anyhow::{Context, Result, bail, ensure};
use rustix::fs::{self, Mode, OFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

const MAX_RECORD: usize = 64 * 1024 * 1024;
const STATE: &str = "state.json";
const FLAGS: OFlags = OFlags::NOFOLLOW
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    checksum: String,
    payload: Value,
}

pub struct Store {
    directory: PathBuf,
    dir: File,
    _lock: File,
    initialized: bool,
}

/// Create a private parent without a path-based chmod or following symlinks.
pub(crate) fn private_directory(path: &Path) -> Result<()> {
    let parent = open_directory(path.parent().context("session parent needs a parent")?)?;
    let name = path.file_name().context("session parent needs a name")?;
    match fs::mkdirat(&parent, name, Mode::from_raw_mode(0o700)) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(error).context("create private session parent"),
    }
    let dir: File = fs::openat(
        &parent,
        name,
        FLAGS | OFlags::RDONLY | OFlags::DIRECTORY,
        Mode::empty(),
    )
    .context("open private session parent without symlinks")?
    .into();
    let meta = dir.metadata()?;
    ensure!(
        meta.is_dir() && meta.uid() == rustix::process::getuid().as_raw(),
        "private session parent must be owned by the current user"
    );
    fs::fchmod(&dir, Mode::from_raw_mode(0o700)).context("secure private session parent")?;
    validate_directory(&dir)?;
    dir.sync_all().context("sync private session parent")?;
    parent.sync_all().context("sync private parent entry")?;
    Ok(())
}

impl Store {
    /// Create one new private session directory. Its parent must exist.
    pub fn create(directory: &Path) -> Result<Self> {
        let name = directory
            .file_name()
            .context("session directory needs a name")?;
        let parent = directory
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = open_directory(parent)?;
        fs::mkdirat(&parent, name, Mode::from_raw_mode(0o700))
            .context("create session directory")?;
        Self::initialize_created(directory, parent)
    }

    /// Reserve a fresh private directory without reopening any existing candidate.
    pub fn create_unique(directory: &Path, prefix: &str) -> Result<Self> {
        Self::create_unique_with(directory, prefix, Self::initialize_created)
    }

    fn create_unique_with(
        directory: &Path,
        prefix: &str,
        initialize: impl FnOnce(&Path, File) -> Result<Self>,
    ) -> Result<Self> {
        ensure!(
            Path::new(prefix).file_name() == Some(std::ffi::OsStr::new(prefix)),
            "session prefix must be one nonempty normal name component"
        );
        let parent = open_directory(directory)?;
        validate_directory(&parent)?;
        for suffix in 0..128 {
            let name = format!("{prefix}-{suffix}");
            match fs::mkdirat(&parent, name.as_str(), Mode::from_raw_mode(0o700)) {
                Ok(()) => return initialize(&directory.join(name), parent),
                Err(rustix::io::Errno::EXIST) => continue,
                Err(error) => return Err(error).context("create session directory"),
            }
        }
        bail!(
            "session directory name allocation exhausted after 128 candidates; start a new invocation"
        )
    }

    fn initialize_created(directory: &Path, parent: File) -> Result<Self> {
        let name = directory
            .file_name()
            .context("session directory needs a name")?;
        let dir: File = fs::openat(
            &parent,
            name,
            FLAGS | OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )?
        .into();
        validate_directory(&dir)?;
        fs::flock(&dir, fs::FlockOperation::NonBlockingLockExclusive)
            .context("session is already open")?;
        let lock: File = fs::openat(
            &dir,
            "lock",
            FLAGS | OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
            Mode::from_raw_mode(0o600),
        )
        .context("create session lock")?
        .into();
        validate_file(&lock)?;
        fs::flock(&lock, fs::FlockOperation::NonBlockingLockExclusive)
            .context("session is already open")?;
        lock.sync_all().context("sync session lock")?;
        dir.sync_all().context("sync session directory")?;
        parent.sync_all().context("sync session parent")?;
        Ok(Self {
            directory: directory.into(),
            dir,
            _lock: lock,
            initialized: false,
        })
    }

    /// Open and validate a committed record while holding its exclusive process lock.
    pub fn open(directory: &Path) -> Result<Self> {
        let dir = open_directory(directory)?;
        validate_directory(&dir)?;
        fs::flock(&dir, fs::FlockOperation::NonBlockingLockExclusive)
            .context("session is already open")?;
        let lock = open_file(&dir, "lock", OFlags::RDWR)?;
        fs::flock(&lock, fs::FlockOperation::NonBlockingLockExclusive)
            .context("session is already open")?;
        let store = Self {
            directory: directory.into(),
            dir,
            _lock: lock,
            initialized: true,
        };
        store.read()?;
        Ok(store)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn state_path(&self) -> PathBuf {
        self.directory.join(STATE)
    }

    pub fn read(&self) -> Result<Value> {
        Self::read_directory(&self.dir)
    }

    /// Read one atomic published snapshot without taking the live writer's lock.
    /// This grants no write authority and never repairs a damaged record.
    pub fn read_snapshot(directory: &Path) -> Result<Value> {
        let dir = open_directory(directory)?;
        validate_directory(&dir)?;
        Self::read_directory(&dir)
    }

    fn read_directory(dir: &File) -> Result<Value> {
        let file = open_file(dir, STATE, OFlags::RDONLY)?;
        ensure!(
            file.metadata()?.len() <= MAX_RECORD as u64,
            "session record exceeds size limit"
        );
        let mut bytes = Vec::new();
        file.take(MAX_RECORD as u64 + 1)
            .read_to_end(&mut bytes)
            .context("read session record")?;
        ensure!(
            bytes.len() <= MAX_RECORD,
            "session record exceeds size limit"
        );
        decode_record(&bytes)
    }

    pub fn write(&mut self, payload: &Value) -> Result<()> {
        self.write_with_directory_sync(payload, File::sync_all)
    }

    fn write_with_directory_sync(
        &mut self,
        payload: &Value,
        sync_directory: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> Result<()> {
        validate_directory(&self.dir)?;
        if self.initialized {
            self.read()?;
        } else {
            match fs::statat(&self.dir, STATE, fs::AtFlags::SYMLINK_NOFOLLOW) {
                Err(rustix::io::Errno::NOENT) => {}
                _ => bail!("new session already has a record"),
            }
        }
        let digest = checksum(payload)?;
        #[derive(Serialize)]
        struct BorrowedRecord<'a> {
            version: u32,
            checksum: String,
            payload: &'a Value,
        }
        let bytes = encode(&BorrowedRecord {
            version: 1,
            checksum: digest,
            payload,
        })?;
        // The encoder accepts nesting that the bounded parser rejects. Prove this
        // exact envelope can be reopened before creating or replacing any file.
        decode_record(&bytes)?;
        // /proc resolves our live directory descriptor, never the caller's replaceable path.
        use std::os::fd::AsRawFd;
        let anchored = PathBuf::from(format!("/proc/self/fd/{}", self.dir.as_raw_fd()));
        let mut temp = tempfile::Builder::new()
            .prefix(".state-")
            .tempfile_in(&anchored)
            .context("create temporary session record")?;
        validate_file(temp.as_file())?;
        temp.write_all(&bytes).context("write session record")?;
        temp.as_file().sync_all().context("sync session record")?;
        // Revalidate immediately before replacement; never knowingly overwrite damaged evidence.
        if self.initialized {
            self.read()?;
        } else {
            ensure!(
                matches!(
                    fs::statat(&self.dir, STATE, fs::AtFlags::SYMLINK_NOFOLLOW),
                    Err(rustix::io::Errno::NOENT)
                ),
                "new session already has a record"
            );
        }
        fs::renameat(
            &self.dir,
            temp.path()
                .file_name()
                .context("temporary record has no name")?,
            &self.dir,
            STATE,
        )
        .context("replace session record")?;
        self.initialized = true;
        sync_directory(&self.dir).context(
            "session record replaced but directory durability uncertain; hold execution",
        )?;
        Ok(())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // A concurrently spawned child can briefly inherit the open file description
        // before exec closes CLOEXEC descriptors. Release ownership explicitly so
        // reopening does not depend on when that unrelated child reaches exec.
        // Closing the descriptors remains the fallback if unlocking fails.
        let _ = fs::flock(&self._lock, fs::FlockOperation::Unlock);
        let _ = fs::flock(&self.dir, fs::FlockOperation::Unlock);
    }
}

fn open_directory(path: &Path) -> Result<File> {
    let mut dir: File = fs::open(
        if path.is_absolute() { "/" } else { "." },
        FLAGS | OFlags::RDONLY | OFlags::DIRECTORY,
        Mode::empty(),
    )?
    .into();
    for part in path.components() {
        match part {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                dir = fs::openat(
                    &dir,
                    name,
                    FLAGS | OFlags::RDONLY | OFlags::DIRECTORY,
                    Mode::empty(),
                )
                .context("open session directory without symlinks")?
                .into();
            }
            _ => bail!("session directory must not contain parent traversal"),
        }
    }
    Ok(dir)
}

fn validate_directory(dir: &File) -> Result<()> {
    let meta = dir.metadata()?;
    ensure!(
        meta.is_dir()
            && meta.uid() == rustix::process::getuid().as_raw()
            && meta.mode() & 0o7777 == 0o700,
        "session directory must be owner-only and owned by the current user"
    );
    Ok(())
}

fn validate_file(file: &File) -> Result<()> {
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.nlink() == 1
            && meta.uid() == rustix::process::getuid().as_raw()
            && meta.mode() & 0o7777 == 0o600,
        "session file must be a single-link owner-only regular file owned by the current user"
    );
    Ok(())
}

fn open_file(dir: &File, name: &str, access: OFlags) -> Result<File> {
    let file: File = fs::openat(dir, name, FLAGS | access, Mode::empty())
        .context("open session file")?
        .into();
    validate_file(&file)?;
    Ok(file)
}

struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_RECORD - self.0.len() {
            return Err(std::io::Error::other("session record exceeds size limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = BoundedBytes(Vec::new());
    serde_json::to_writer(&mut bytes, value)
        .map_err(|_| anyhow::anyhow!("session record cannot be encoded within size limit"))?;
    Ok(bytes.0)
}
fn checksum(payload: &Value) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(encode(payload)?)))
}

fn decode_record(bytes: &[u8]) -> Result<Value> {
    // Parser errors may quote sensitive input, so never retain them as error causes.
    let record: Record =
        serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("invalid session record"))?;
    ensure!(record.version == 1, "unsupported session record version");
    ensure!(
        checksum(&record.payload)? == record.checksum,
        "session record checksum mismatch"
    );
    Ok(record.payload)
}

#[cfg(test)]
mod durability_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn directory_sync_failure_reports_uncertainty_with_valid_replacement() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("session");
        let mut store = Store::create(&directory).unwrap();
        store.write(&json!("before")).unwrap();
        let error = store
            .write_with_directory_sync(&json!("after"), |_| {
                Err(std::io::Error::other("injected directory sync failure"))
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("record replaced but directory durability uncertain")
        );
        assert_eq!(store.read().unwrap(), json!("after"));
        drop(store);
        assert_eq!(
            Store::open(&directory).unwrap().read().unwrap(),
            json!("after")
        );
    }
}

#[cfg(test)]
mod unique_directory_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn unique_creation_preserves_existing_record_and_skips_symlink_without_following() {
        let root = tempfile::tempdir().unwrap();
        private_directory(root.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("marker"), "unchanged").unwrap();
        let first = root.path().join("same-clock-0");
        let mut existing = Store::create(&first).unwrap();
        existing
            .write(&serde_json::json!({"retained":true}))
            .unwrap();
        let before = std::fs::read(first.join(STATE)).unwrap();
        let link = root.path().join("same-clock-1");
        symlink(outside.path(), &link).unwrap();
        let mut created = Store::create_unique(root.path(), "same-clock").unwrap();
        assert_eq!(created.directory(), root.path().join("same-clock-2"));
        assert_eq!(
            std::fs::metadata(created.directory())
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );
        created.write(&serde_json::json!({"new":true})).unwrap();
        assert!(Store::open(created.directory()).is_err());
        assert!(
            Store::create(&first).is_err(),
            "exact-name creation reused an existing record"
        );
        assert_eq!(std::fs::read(first.join(STATE)).unwrap(), before);
        assert_eq!(std::fs::read_link(&link).unwrap(), outside.path());
        assert_eq!(
            std::fs::read_to_string(outside.path().join("marker")).unwrap(),
            "unchanged"
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
    }

    #[test]
    fn unique_creation_bounds_collisions_and_refuses_invalid_prefixes() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("sessions");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let absolute = root.path().join("absolute");
        for prefix in [
            "",
            ".",
            "..",
            "../escape",
            absolute.to_str().unwrap(),
            "nested/path",
            "nested/",
        ] {
            assert!(
                Store::create_unique(&parent, prefix).is_err(),
                "invalid prefix accepted: {prefix}"
            );
            assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        }
        for suffix in 0..128 {
            std::fs::create_dir(parent.join(format!("occupied-{suffix}"))).unwrap();
        }
        let error = Store::create_unique(&parent, "occupied").err().unwrap();
        assert!(error.to_string().contains("128"));
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 128);
        assert!(!parent.join("occupied-128").exists());
    }

    #[test]
    fn unique_creation_never_retries_initialization_already_exists_errors() {
        let root = tempfile::tempdir().unwrap();
        private_directory(root.path()).unwrap();
        let mut initialized = 0;
        let error = Store::create_unique_with(root.path(), "owned", |directory, parent| {
            initialized += 1;
            // Produce the real lock-file EEXIST after successful mkdirat.
            std::fs::write(directory.join("lock"), "retained collision evidence")?;
            Store::initialize_created(directory, parent)
        })
        .err()
        .unwrap();
        assert_eq!(initialized, 1);
        assert!(format!("{error:#}").contains("create session lock"));
        assert_eq!(
            error.downcast_ref::<rustix::io::Errno>(),
            Some(&rustix::io::Errno::EXIST)
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("owned-0/lock")).unwrap(),
            "retained collision evidence"
        );
        assert!(!root.path().join("owned-1").exists());
    }

    #[test]
    fn unique_creation_propagates_other_mkdir_errors_before_initialization() {
        let root = tempfile::tempdir().unwrap();
        private_directory(root.path()).unwrap();
        let error = Store::create_unique_with(root.path(), &"a".repeat(300), |_, _| {
            panic!("failed directory reservation reached initialization")
        })
        .err()
        .unwrap();
        assert_eq!(
            error.downcast_ref::<rustix::io::Errno>(),
            Some(&rustix::io::Errno::NAMETOOLONG)
        );
        assert!(format!("{error:#}").contains("create session directory"));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn unique_creation_keeps_the_pinned_parent_when_its_path_is_replaced() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent_path = root.path().join("sessions");
        let moved = root.path().join("moved");
        std::fs::create_dir(&parent_path).unwrap();
        std::fs::set_permissions(&parent_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut created = Store::create_unique_with(&parent_path, "pinned", |directory, parent| {
            std::fs::rename(&parent_path, &moved)?;
            symlink(outside.path(), &parent_path)?;
            Store::initialize_created(directory, parent)
        })
        .unwrap();
        created.write(&serde_json::json!({"pinned":true})).unwrap();
        assert_eq!(
            Store::read_snapshot(&moved.join("pinned-0")).unwrap()["pinned"],
            true
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
