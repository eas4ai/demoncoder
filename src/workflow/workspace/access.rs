//! Original Linux object access metadata. Global/mount/remote policy is not an
//! object property and must remain an external precondition in the admission key.
//! Materializers must restore these values or hold; byte/chmod copying is not enough.
use anyhow::{Result, ensure};
use rustix::fs::{AtFlags, Mode, OFlags, StatxFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{File, Metadata},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
};

const MAX_ATTRIBUTE_BYTES: usize = 65536;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessMetadata {
    pub uid: u32,
    pub gid: u32,
    pub acl: AclMetadata,
    /// Opaque exposed security/system/trusted attributes, including labels and
    /// capabilities. Ordinary user.* attributes are not access policy inputs.
    pub extended: BTreeMap<String, Vec<u8>>,
    pub attributes: u64,
    pub supported_attributes: u64,
}
impl std::fmt::Debug for AccessMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessMetadata")
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .field("acl", &self.acl)
            .field("extended_count", &self.extended.len())
            .field("attributes", &self.attributes)
            .field("supported_attributes", &self.supported_attributes)
            .finish()
    }
}
impl AccessMetadata {
    /// Filesystems may support different inactive features. Reproduction still
    /// requires identical active policy and destination support for every active
    /// source attribute. Capture/freshness equality keeps the complete mask.
    fn reproduced_by(&self, actual: &Self) -> bool {
        self.uid == actual.uid
            && self.gid == actual.gid
            && self.acl == actual.acl
            && self.extended == actual.extended
            && self.attributes == actual.attributes
            && actual.supported_attributes & self.attributes == self.attributes
    }

    pub(super) fn byte_count(&self) -> usize {
        let acl = match &self.acl {
            AclMetadata::SymlinkNotApplicable => 0,
            AclMetadata::Posix { access, default } => {
                access.as_ref().map_or(0, Vec::len) + default.as_ref().map_or(0, Vec::len)
            }
        };
        64 + acl
            + self
                .extended
                .iter()
                .map(|(name, value)| name.len() + value.len())
                .sum::<usize>()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AclMetadata {
    /// Linux symlink POSIX ACLs do not apply. Security labels, when exposed, are
    /// captured separately using no-follow calls relative to a pinned parent.
    SymlinkNotApplicable,
    /// None is a successful ENODATA query, distinct from an unsupported query.
    /// A default ACL applies only to directories (the entry kind records that).
    Posix {
        access: Option<Vec<u8>>,
        default: Option<Vec<u8>>,
    },
}
impl std::fmt::Debug for AclMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SymlinkNotApplicable => f.write_str("SymlinkNotApplicable"),
            Self::Posix { access, default } => f
                .debug_struct("Posix")
                .field("access_present", &access.is_some())
                .field("default_present", &default.is_some())
                .finish(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum AccessCaptureError {
    Query {
        operation: &'static str,
        errno: rustix::io::Errno,
    },
    Changed,
    UnsupportedAttributeName,
    Oversized,
}
impl std::fmt::Display for AccessCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Query { operation, errno } => write!(
                f,
                "required access-metadata {operation} query failed: {errno}"
            ),
            Self::Changed => f.write_str("access metadata changed during capture"),
            Self::UnsupportedAttributeName => {
                f.write_str("unsupported non-UTF-8 access attribute name")
            }
            Self::Oversized => f.write_str("access metadata exceeds the bounded object allowance"),
        }
    }
}
impl std::error::Error for AccessCaptureError {}

pub(super) fn capture(
    file: &File,
    metadata: &Metadata,
    checkpoint: &dyn Fn() -> Result<()>,
) -> Result<AccessMetadata> {
    let before = super::stamp(file)?;
    let captured = capture_from(file, metadata, &Source::Descriptor(file), checkpoint)?;
    ensure!(super::stamp(file)? == before, AccessCaptureError::Changed);
    Ok(captured)
}

pub(super) fn capture_symlink(
    file: &File,
    metadata: &Metadata,
    parent: &File,
    leaf: &OsStr,
    checkpoint: &dyn Fn() -> Result<()>,
) -> Result<AccessMetadata> {
    let source = Source::Symlink { parent, leaf };
    let parent_before = super::stamp(parent)?;
    let before = super::stamp(file)?;
    ensure!(source.stamp()? == before, AccessCaptureError::Changed);
    let result = capture_from(file, metadata, &source, checkpoint)?;
    ensure!(
        source.stamp()? == before
            && super::stamp(file)? == before
            && super::stamp(parent)? == parent_before,
        AccessCaptureError::Changed
    );
    Ok(result)
}

// The leaf is supplied only by the scanner's already validated single component.
// l* xattr syscalls follow the live procfs parent descriptor, NEVER the leaf link.
// Keeping parent open and checking the leaf around queries rejects replacement;
// no workspace-controlled intermediate name can redirect a metadata read.
enum Source<'a> {
    Descriptor(&'a File),
    Symlink { parent: &'a File, leaf: &'a OsStr },
}
impl Source<'_> {
    fn leaf_path(&self) -> std::path::PathBuf {
        match self {
            Self::Symlink { parent, leaf } => {
                std::path::Path::new(&format!("/proc/self/fd/{}", parent.as_raw_fd())).join(leaf)
            }
            Self::Descriptor(_) => unreachable!("descriptor sources use f* xattr queries"),
        }
    }
    fn stamp(&self) -> Result<super::Stamp> {
        match self {
            Self::Descriptor(file) => super::stamp(file),
            Self::Symlink { parent, leaf } => {
                let file: File = rustix::fs::openat2(
                    parent,
                    *leaf,
                    OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                    super::RESOLVE,
                )?
                .into();
                super::stamp(&file)
            }
        }
    }
    fn query(&self, name: &str, bytes: &mut [u8]) -> rustix::io::Result<usize> {
        match self {
            Self::Descriptor(file) => rustix::fs::fgetxattr(file, name, bytes),
            Self::Symlink { .. } => rustix::fs::lgetxattr(self.leaf_path(), name, bytes),
        }
    }
    fn list(&self) -> Result<Vec<u8>> {
        let mut bytes = vec![0; MAX_ATTRIBUTE_BYTES];
        let count = match self {
            Self::Descriptor(file) => rustix::fs::flistxattr(file, bytes.as_mut_slice()),
            Self::Symlink { .. } => rustix::fs::llistxattr(self.leaf_path(), bytes.as_mut_slice()),
        }
        .map_err(|errno| AccessCaptureError::Query {
            operation: "attribute-list",
            errno,
        })?;
        bytes.truncate(count);
        // xattr order is unspecified, so compare canonical lists around capture.
        let mut names: Vec<_> = bytes
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .collect();
        names.sort();
        let canonical: Vec<u8> = names
            .into_iter()
            .flat_map(|name| name.iter().copied().chain([0]))
            .collect();
        Ok(canonical.into_boxed_slice().into_vec())
    }
}

fn capture_from(
    file: &File,
    metadata: &Metadata,
    source: &Source<'_>,
    checkpoint: &dyn Fn() -> Result<()>,
) -> Result<AccessMetadata> {
    checkpoint()?;
    let flags = rustix::fs::statx(
        file,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS,
    )
    .map_err(|errno| AccessCaptureError::Query {
        operation: "statx",
        errno,
    })?;
    let list = source.list()?;
    checkpoint()?;
    let acl = if metadata.file_type().is_symlink() {
        AclMetadata::SymlinkNotApplicable
    } else {
        AclMetadata::Posix {
            access: query(source, "system.posix_acl_access", true)?,
            default: if metadata.is_dir() {
                query(source, "system.posix_acl_default", true)?
            } else {
                None
            },
        }
    };
    let mut captured = AccessMetadata {
        uid: metadata.uid(),
        gid: metadata.gid(),
        acl,
        extended: BTreeMap::new(),
        attributes: flags.stx_attributes.bits(),
        supported_attributes: flags.stx_attributes_mask.bits(),
    };
    for name in list
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        checkpoint()?;
        if name.starts_with(b"user.") {
            continue;
        }
        if !metadata.file_type().is_symlink()
            && matches!(
                name,
                b"system.posix_acl_access" | b"system.posix_acl_default"
            )
        {
            continue;
        }
        let name =
            std::str::from_utf8(name).map_err(|_| AccessCaptureError::UnsupportedAttributeName)?;
        let bytes = query(source, name, false)?.ok_or(AccessCaptureError::Changed)?;
        captured.extended.insert(name.to_owned(), bytes);
        ensure!(
            captured.byte_count() <= MAX_ATTRIBUTE_BYTES,
            AccessCaptureError::Oversized
        );
    }
    ensure!(
        captured.byte_count() <= MAX_ATTRIBUTE_BYTES,
        AccessCaptureError::Oversized
    );
    checkpoint()?;
    ensure!(source.list()? == list, AccessCaptureError::Changed);
    Ok(captured)
}

fn query(source: &Source<'_>, name: &str, allow_absent: bool) -> Result<Option<Vec<u8>>> {
    let mut bytes = vec![0; MAX_ATTRIBUTE_BYTES];
    match source.query(name, bytes.as_mut_slice()) {
        Ok(count) => {
            bytes.truncate(count);
            // The fixed syscall buffer must not become retained spare capacity:
            // the object/global metadata allowances account for actual material.
            Ok(Some(bytes.into_boxed_slice().into_vec()))
        }
        Err(rustix::io::Errno::NODATA) if allow_absent => Ok(None),
        Err(errno) => Err(AccessCaptureError::Query {
            operation: "attribute",
            errno,
        }
        .into()),
    }
}

/// Restore a private materialized object and compare every retained policy input.
/// Ownership precedes mode and capabilities because chown can clear those bits.
pub(crate) fn restore_and_verify(
    file: &File,
    link: Option<(&File, &OsStr)>,
    mode: u32,
    expected: &AccessMetadata,
    checkpoint: &dyn Fn() -> Result<()>,
) -> Result<()> {
    use anyhow::Context;
    use rustix::process::{Gid, Uid};
    checkpoint()?;
    let before = file.metadata()?;
    if before.uid() != expected.uid || before.gid() != expected.gid {
        let owner = Some(Uid::from_raw(expected.uid));
        let group = Some(Gid::from_raw(expected.gid));
        match link {
            Some((parent, leaf)) => {
                rustix::fs::chownat(parent, leaf, owner, group, AtFlags::SYMLINK_NOFOLLOW)
            }
            None => rustix::fs::fchown(file, owner, group),
        }
        .context("snapshot ownership cannot be reproduced")?;
    }
    if link.is_none() {
        rustix::fs::fchmod(file, Mode::from_raw_mode(mode))
            .context("snapshot mode cannot be reproduced")?;
    }
    let source = match link {
        Some((parent, leaf)) => Source::Symlink { parent, leaf },
        None => Source::Descriptor(file),
    };
    let mut attributes = expected.extended.clone();
    if let AclMetadata::Posix { access, default } = &expected.acl {
        for (name, bytes) in [
            ("system.posix_acl_access", access),
            ("system.posix_acl_default", default),
        ] {
            if let Some(bytes) = bytes {
                attributes.insert(name.into(), bytes.clone());
            }
        }
    }
    // Remove inherited policy attributes not present in the retained object.
    let names = source.list()?;
    for name in names
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty() && !name.starts_with(b"user."))
    {
        checkpoint()?;
        let name = std::str::from_utf8(name)?;
        if !attributes.contains_key(name) {
            match &source {
                Source::Descriptor(file) => rustix::fs::fremovexattr(file, name),
                Source::Symlink { .. } => rustix::fs::lremovexattr(source.leaf_path(), name),
            }
            .context("inherited snapshot access attribute cannot be removed")?;
        }
    }
    for (name, bytes) in attributes {
        checkpoint()?;
        match &source {
            Source::Descriptor(file) => {
                rustix::fs::fsetxattr(file, name.as_str(), &bytes, rustix::fs::XattrFlags::empty())
            }
            Source::Symlink { .. } => rustix::fs::lsetxattr(
                source.leaf_path(),
                name.as_str(),
                &bytes,
                rustix::fs::XattrFlags::empty(),
            ),
        }
        .context("snapshot ACL or access attribute cannot be reproduced")?;
    }
    let metadata = file.metadata()?;
    let actual = match link {
        Some((parent, leaf)) => capture_symlink(file, &metadata, parent, leaf, checkpoint)?,
        None => capture(file, &metadata, checkpoint)?,
    };
    ensure!(
        metadata.mode() & 0o7777 == mode & 0o7777,
        "snapshot mode cannot be reproduced"
    );
    ensure!(
        expected.reproduced_by(&actual),
        "snapshot access metadata or statx semantics cannot be reproduced"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn materialization_accepts_different_inactive_filesystem_features() {
        let root = tempfile::tempdir().unwrap();
        let file = File::create(root.path().join("file")).unwrap();
        let metadata = file.metadata().unwrap();
        let original = capture(&file, &metadata, &|| Ok(())).unwrap();
        let mut retained = original.clone();
        retained.supported_attributes ^= 1u64 << 63;
        assert_ne!(retained, original, "freshness must retain the full mask");
        restore_and_verify(&file, None, metadata.mode(), &retained, &|| Ok(())).unwrap();
    }

    #[test]
    fn reproduction_rejects_changed_access_and_unsupported_active_attributes() {
        let retained = AccessMetadata {
            uid: 1000,
            gid: 1000,
            acl: AclMetadata::Posix {
                access: None,
                default: None,
            },
            extended: BTreeMap::new(),
            attributes: 0x4,
            supported_attributes: 0x34,
        };
        let mut reproduced = retained.clone();
        reproduced.supported_attributes = 0x14;
        assert!(retained.reproduced_by(&reproduced));
        assert_ne!(
            retained, reproduced,
            "freshness must retain feature support"
        );
        let reject = |label, change: fn(&mut AccessMetadata)| {
            let mut actual = reproduced.clone();
            change(&mut actual);
            assert!(!retained.reproduced_by(&actual), "accepted changed {label}");
        };
        reject("owner", |actual| actual.uid += 1);
        reject("group", |actual| actual.gid += 1);
        reject("ACL", |actual| {
            actual.acl = AclMetadata::Posix {
                access: Some(vec![1]),
                default: None,
            };
        });
        reject("security attribute", |actual| {
            actual
                .extended
                .insert("security.synthetic".into(), b"synthetic-policy".to_vec());
        });
        reject("missing active flag", |actual| actual.attributes = 0);
        reject("additional active flag", |actual| actual.attributes |= 0x10);
        reject("unsupported active flag", |actual| {
            actual.supported_attributes &= !0x4;
        });
    }

    #[test]
    fn canonical_attribute_name_buffer_retains_no_spare_capacity() {
        let root = tempfile::tempdir().unwrap();
        let file = File::create(root.path().join("file")).unwrap();
        rustix::fs::fsetxattr(
            &file,
            "user.aaaaaaaa",
            b"value",
            rustix::fs::XattrFlags::empty(),
        )
        .unwrap();
        rustix::fs::fsetxattr(&file, "user.z", b"value", rustix::fs::XattrFlags::empty()).unwrap();
        let names = Source::Descriptor(&file).list().unwrap();
        assert!(names.windows(13).any(|name| name == b"user.aaaaaaaa"));
        eprintln!(
            "canonical attribute names: {} bytes; retained buffer capacity: {} bytes",
            names.len(),
            names.capacity()
        );
        assert_eq!(names.capacity(), names.len());
    }

    #[test]
    fn failed_acl_query_is_not_absence() {
        let root = tempfile::tempdir().unwrap();
        let file: File = rustix::fs::openat2(
            rustix::fs::CWD,
            root.path(),
            OFlags::PATH | OFlags::CLOEXEC,
            Mode::empty(),
            rustix::fs::ResolveFlags::NO_SYMLINKS,
        )
        .unwrap()
        .into();
        let error = query(&Source::Descriptor(&file), "system.posix_acl_access", true).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<AccessCaptureError>(),
            Some(AccessCaptureError::Query {
                errno: rustix::io::Errno::BADF,
                ..
            })
        ));
        assert!(!error.to_string().contains(root.path().to_str().unwrap()));
        let readable = File::open(root.path()).unwrap();
        assert!(
            query(
                &Source::Descriptor(&readable),
                "system.posix_acl_access",
                true
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn symlink_xattrs_never_follow_private_target_and_replacement_holds() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("private");
        std::fs::write(&target, b"private-canary").unwrap();
        let target_file = File::open(&target).unwrap();
        rustix::fs::fsetxattr(
            &target_file,
            "user.private_canary",
            b"private-metadata",
            rustix::fs::XattrFlags::empty(),
        )
        .unwrap();
        std::os::unix::fs::symlink(&target, root.path().join("link")).unwrap();
        let parent = File::open(root.path()).unwrap();
        let leaf = OsStr::new("link");
        let link: File = rustix::fs::openat2(
            &parent,
            leaf,
            OFlags::PATH | OFlags::NOFOLLOW,
            Mode::empty(),
            super::super::RESOLVE,
        )
        .unwrap()
        .into();
        let source = Source::Symlink {
            parent: &parent,
            leaf,
        };
        let list = source.list().unwrap();
        assert!(
            !list
                .windows(b"user.private_canary".len())
                .any(|part| part == b"user.private_canary")
        );
        let metadata = link.metadata().unwrap();
        assert!(matches!(
            capture_symlink(&link, &metadata, &parent, leaf, &|| Ok(()))
                .unwrap()
                .acl,
            AclMetadata::SymlinkNotApplicable
        ));
        let changed = Cell::new(false);
        let error = capture_symlink(&link, &metadata, &parent, leaf, &|| {
            if !changed.replace(true) {
                std::fs::remove_file(root.path().join("link"))?;
                std::os::unix::fs::symlink("replacement", root.path().join("link"))?;
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.downcast_ref::<AccessCaptureError>().is_some());
    }

    #[test]
    fn capture_time_file_mutation_is_rejected_by_production_metadata_capture() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        std::fs::write(&path, b"original").unwrap();
        let file = File::open(&path).unwrap();
        let metadata = file.metadata().unwrap();
        let changed = Cell::new(false);
        let error = capture(&file, &metadata, &|| {
            if !changed.replace(true) {
                std::fs::write(&path, b"changed during capture")?;
            }
            Ok(())
        })
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<AccessCaptureError>(),
            Some(AccessCaptureError::Changed)
        ));
    }
}
