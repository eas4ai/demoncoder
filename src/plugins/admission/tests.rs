use super::*;
use crate::{
    config::Connection,
    workflow::runtime::{Identity, Record},
};
use serde_json::json;
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn cancelled_rescan_retains_boundary_until_worker_stops_and_never_releases_effect() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("workspace");
    std::fs::create_dir(&path).unwrap();
    let bytes = vec![b'a'; 1024 * 1024];
    for i in 0..15 {
        std::fs::write(path.join(format!("file-{i}")), &bytes).unwrap();
    }
    let workspace = Arc::new(GateWorkspace::open(&path).unwrap());
    let snapshot = Arc::new(
        workspace
            .capture(&GateReadSet::default(), &AtomicBool::new(false))
            .unwrap(),
    );
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let record:Record=serde_json::from_value(json!({"workspace":path,"identity":Identity::from(&connection),"archived":[],"next_task":1,"checkpoint_cursor":0,"operations":[],"messages":[],"recovery_pending":false,"decisions":[]})).unwrap();
    let runtime = SharedRuntime::for_test(&root.path().join("record"), record).unwrap();
    let boundary = runtime.mutation_boundary(snapshot.root_identity()).unwrap();
    let guard = Arc::new(boundary.clone().lock_owned().await);
    let captures = Arc::new(tokio::sync::Semaphore::new(1));
    let admitted = AdmittedCandidate {
        target: None,
        workspace,
        snapshots: vec![snapshot],
        runtime: runtime.clone(),
        operation: 1,
        captures: captures.clone(),
    };
    let output = path.join("released");
    let effect = output.clone();
    let task = tokio::spawn(async move {
        admitted.validate(Some(guard)).await.unwrap();
        std::fs::write(effect, "must not run").unwrap();
    });
    let started = tokio::time::timeout(Duration::from_secs(5), async {
        while captures.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    task.abort();
    let joined = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("cancelled owner did not stop");
    started.expect("rescan did not start");
    assert!(joined.unwrap_err().is_cancelled());
    let _guard = tokio::time::timeout(Duration::from_secs(5), boundary.lock())
        .await
        .expect("cancelled scan retained mutation boundary");
    assert_eq!(
        captures.available_permits(),
        1,
        "boundary released before cancelled reader stopped"
    );
    assert!(!output.exists());
    assert!(runtime.record().unwrap().operations.is_empty());
}
