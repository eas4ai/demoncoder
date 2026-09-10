use super::*;

#[test]
fn actual_revision_function_binds_uid_gid_acl_and_exposed_metadata() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"same").unwrap();
    let baseline = capture(root.path()).unwrap();
    assert_eq!(baseline.digest, baseline.calculate_digest().unwrap());
    for field in 0..5 {
        let mut changed = baseline.clone();
        let metadata = changed
            .entries
            .get_mut("./file")
            .unwrap()
            .access
            .as_mut()
            .unwrap();
        match field {
            0 => metadata.uid += 1,
            1 => metadata.gid += 1,
            2 => {
                metadata.acl = AclMetadata::Posix {
                    access: Some(vec![1, 2, 3]),
                    default: None,
                }
            }
            3 => {
                metadata
                    .extended
                    .insert("security.synthetic".into(), vec![9]);
            }
            _ => metadata.attributes ^= rustix::fs::StatxAttributes::IMMUTABLE.bits(),
        }
        assert_ne!(
            baseline.digest,
            changed.calculate_digest().unwrap(),
            "field {field}"
        );
    }
}

#[test]
fn metadata_budget_cannot_be_exceeded_by_actual_capture() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"same").unwrap();
    let file = File::open(root.path()).unwrap();
    let scope = CaptureScope::default();
    let mut scan = Scan {
        root: &file,
        started: Instant::now(),
        entries: BTreeMap::new(),
        stamps: BTreeMap::new(),
        raw: None,
        cancelled: None,
        scope: &scope,
        bytes: 0,
        selection: None,
        memberships: BTreeMap::new(),
        examined: 0,
        metadata_bytes: MAX_METADATA_BYTES,
    };
    assert!(
        scan.walk(Path::new("./file"), 1)
            .unwrap_err()
            .to_string()
            .contains("metadata")
    );
}
