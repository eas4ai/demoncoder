#[path = "../src/workflow/store.rs"]
mod store;
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};
use store::Store;

#[test]
fn published_snapshot_is_readable_during_live_writer_and_rejects_damage() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let mut writer = Store::create(&path).unwrap();
    writer.write(&json!({"generation":1})).unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(
        Store::read_snapshot(&path).unwrap(),
        json!({"generation":1})
    );
    writer.write(&json!({"generation":2})).unwrap();
    assert_eq!(
        Store::read_snapshot(&path).unwrap(),
        json!({"generation":2})
    );
    fs::write(path.join("state.json"), "damaged private evidence").unwrap();
    assert!(Store::read_snapshot(&path).is_err());
}

#[test]
fn private_parent_rejects_links_without_changing_target_permissions() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
    let path = root.path().join("private");
    symlink(&outside, &path).unwrap();
    assert!(store::private_directory(&path).is_err());
    assert_eq!(fs::metadata(&outside).unwrap().mode() & 0o777, 0o755);
    fs::remove_file(&path).unwrap();
    store::private_directory(&path).unwrap();
    store::private_directory(&path).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    store::private_directory(&path).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
}

#[test]
fn roundtrip_private_and_exclusive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let mut store = Store::create(&path).unwrap();
    assert_eq!(store.directory(), path);
    assert!(Store::open(&path).is_err());
    store.write(&json!({"conversation": ["private"]})).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
    for name in ["state.json", "lock"] {
        assert_eq!(fs::metadata(path.join(name)).unwrap().mode() & 0o777, 0o600);
    }
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.read().unwrap(), json!({"conversation": ["private"]}));
}

#[test]
fn refuses_broken_records_without_echoing_contents() {
    for bytes in [
        b"{private".as_slice(),
        br#"{"version":99,"checksum":"private","payload":null}"#,
        br#"{"version":1,"checksum":"private","payload":null}"#,
        br#"{"version":1,"checksum":"private","payload":null,"extra":true}"#,
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session");
        let mut store = Store::create(&path).unwrap();
        store.write(&json!(null)).unwrap();
        fs::write(path.join("state.json"), bytes).unwrap();
        let error = store.read().unwrap_err();
        assert!(!format!("{error:#}").contains("private"));
        assert!(store.write(&json!("replacement")).is_err());
        assert_eq!(fs::read(path.join("state.json")).unwrap(), bytes);
        drop(store);
        assert!(Store::open(&path).is_err());
    }
}

#[test]
fn refuses_missing_record_and_unsafe_paths() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    drop(Store::create(&path).unwrap());
    assert!(Store::open(&path).is_err());
    assert!(Store::create(&path).is_err());
    symlink(&path, root.path().join("link")).unwrap();
    assert!(Store::open(&root.path().join("link")).is_err());
    assert!(Store::create(&root.path().join("link/child")).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Store::open(&path).is_err());
}

#[test]
fn rejects_state_links_and_preserves_outside_file() {
    for hard in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, "unchanged").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        let path = root.path().join("session");
        let mut store = Store::create(&path).unwrap();
        store.write(&json!("before")).unwrap();
        fs::remove_file(path.join("state.json")).unwrap();
        if hard {
            fs::hard_link(&outside, path.join("state.json")).unwrap();
        } else {
            symlink(&outside, path.join("state.json")).unwrap();
        }
        assert!(store.read().is_err());
        assert!(store.write(&json!("after")).is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "unchanged");
    }
}

#[test]
fn rejects_unsafe_lock_files() {
    for kind in ["symlink", "hardlink", "directory", "fifo", "public"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session");
        let mut store = Store::create(&path).unwrap();
        store.write(&json!(null)).unwrap();
        drop(store);
        let lock = path.join("lock");
        fs::remove_file(&lock).unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, "unchanged").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        match kind {
            "symlink" => symlink(&outside, &lock).unwrap(),
            "hardlink" => fs::hard_link(&outside, &lock).unwrap(),
            "directory" => fs::create_dir(&lock).unwrap(),
            "fifo" => rustix::fs::mknodat(
                rustix::fs::CWD,
                &lock,
                rustix::fs::FileType::Fifo,
                rustix::fs::Mode::from_raw_mode(0o600),
                0,
            )
            .unwrap(),
            _ => {
                fs::write(&lock, "").unwrap();
                fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
        assert!(Store::open(&path).is_err(), "{kind}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "unchanged");
    }
}

#[test]
fn rejects_nonregular_and_oversized_records() {
    for kind in ["directory", "fifo", "oversize", "public"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session");
        let mut store = Store::create(&path).unwrap();
        store.write(&json!(null)).unwrap();
        let state = path.join("state.json");
        fs::remove_file(&state).unwrap();
        match kind {
            "directory" => fs::create_dir(&state).unwrap(),
            "fifo" => rustix::fs::mknodat(
                rustix::fs::CWD,
                &state,
                rustix::fs::FileType::Fifo,
                rustix::fs::Mode::from_raw_mode(0o600),
                0,
            )
            .unwrap(),
            "oversize" => {
                let f = fs::File::create(&state).unwrap();
                f.set_permissions(fs::Permissions::from_mode(0o600))
                    .unwrap();
                f.set_len(64 * 1024 * 1024 + 1).unwrap();
            }
            _ => {
                fs::write(&state, "null").unwrap();
                fs::set_permissions(&state, fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
        assert!(store.read().is_err(), "{kind}");
        assert!(store.write(&json!("replacement")).is_err(), "{kind}");
    }
}

#[test]
fn failed_oversized_write_preserves_previous_record() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let mut store = Store::create(&path).unwrap();
    store.write(&json!("before")).unwrap();
    let previous = fs::read(path.join("state.json")).unwrap();
    assert!(
        store
            .write(&json!("x".repeat(64 * 1024 * 1024 - 50)))
            .is_err()
    );
    assert_eq!(fs::read(path.join("state.json")).unwrap(), previous);
    assert_eq!(store.read().unwrap(), json!("before"));
}

#[test]
fn pinned_directory_survives_path_replacement_and_ignores_uncommitted_temps() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let moved = root.path().join("moved");
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let mut store = Store::create(&path).unwrap();
    store.write(&json!("before")).unwrap();
    fs::write(path.join(".state-interrupted"), "invalid incomplete bytes").unwrap();
    fs::rename(&path, &moved).unwrap();
    symlink(&outside, &path).unwrap();
    store.write(&json!("after")).unwrap();
    assert_eq!(store.read().unwrap(), json!("after"));
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
    drop(store);
    assert_eq!(Store::open(&moved).unwrap().read().unwrap(), json!("after"));
}

#[test]
fn changing_lock_name_does_not_allow_two_owners() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let mut store = Store::create(&path).unwrap();
    store.write(&json!(null)).unwrap();
    fs::rename(path.join("lock"), path.join("old-lock")).unwrap();
    fs::write(path.join("lock"), "").unwrap();
    fs::set_permissions(path.join("lock"), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Store::open(&path).is_err());
}

#[test]
fn crash_lock_child() {
    let Some(path) = std::env::var_os("DEMONCODER_STORE_CRASH_TEST") else {
        return;
    };
    let mut store = Store::create(std::path::Path::new(&path)).unwrap();
    store.write(&json!("survives crash")).unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn process_death_releases_lock() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_lock_child"])
        .env("DEMONCODER_STORE_CRASH_TEST", &path)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !path.join("state.json").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let exists = path.join("state.json").exists();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(exists, "child did not commit a record");
    assert_eq!(
        Store::open(&path).unwrap().read().unwrap(),
        json!("survives crash")
    );
}

#[test]
fn rejects_independently_mutated_valid_envelopes() {
    for change in ["version", "unknown", "checksum", "payload"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("session");
        let mut store = Store::create(&path).unwrap();
        store.write(&json!({"message": "private"})).unwrap();
        let state = path.join("state.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
        match change {
            "version" => record["version"] = json!(99),
            "unknown" => record["extra"] = json!(true),
            "checksum" => record["checksum"] = json!("00000000"),
            _ => record["payload"]["message"] = json!("different"),
        }
        fs::write(&state, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(store.read().is_err(), "{change}");
    }
}

#[test]
fn rejects_unreadable_nesting_before_replacing_previous_record() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session");
    let mut store = Store::create(&path).unwrap();
    store.write(&json!("before")).unwrap();
    let previous = fs::read(path.join("state.json")).unwrap();
    let mut nested = json!(null);
    for _ in 0..130 {
        nested = json!([nested]);
    }
    assert!(store.write(&nested).is_err());
    assert_eq!(fs::read(path.join("state.json")).unwrap(), previous);
    assert_eq!(store.read().unwrap(), json!("before"));
    drop(store);
    assert_eq!(Store::open(&path).unwrap().read().unwrap(), json!("before"));
}
