use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use anyhow::Result;
use demoncoder::{
    events::{Event, EventSink},
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolExtension},
};
use serde_json::{Value, json};

struct Extension(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl ToolExtension for Extension {
    fn definitions(&self) -> Vec<Value> {
        vec![
            json!({"name":"delegate", "description":"Assign work", "input_schema":{"type":"object"}}),
        ]
    }
    async fn execute(&self, call: &ToolCall, _: &EventSink) -> Result<String> {
        assert_eq!(call.name, "delegate");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("assignment retained".into())
    }
}

#[tokio::test]
async fn parent_extension_uses_shared_receipts_and_rejects_unknown_tools() {
    let root = tempfile::tempdir().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let policy = AccessPolicy {
        extension: Some(Arc::new(Extension(count.clone()))),
        ..AccessPolicy::default()
    };
    let executor = ToolExecutor::with_policy(root.path(), &policy).unwrap();
    assert_eq!(
        ToolExecutor::new(root.path()).unwrap().definitions().len(),
        4
    );
    assert_eq!(executor.definitions().len(), 5);
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let events = EventSink::new("parent".into(), tx, None).unwrap();
    let call = ToolCall {
        id: "one".into(),
        name: "delegate".into(),
        arguments: json!({}),
    };
    let result = executor.execute(call.clone(), &events).await.unwrap();
    assert!(result.success);
    assert_eq!(result.output, "assignment retained");
    assert!(matches!(
        rx.recv().await.unwrap().event,
        Event::ToolStarted { .. }
    ));
    assert!(
        matches!(rx.recv().await.unwrap().event, Event::ToolFinished { result } if result.success)
    );
    assert!(rx.try_recv().is_err());
    let result = executor
        .execute(
            ToolCall {
                name: "integrate".into(),
                ..call
            },
            &events,
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn child_and_reviewer_cannot_receive_parent_extensions() {
    let root = tempfile::tempdir().unwrap();
    for mut policy in [
        AccessPolicy::worktree_only(vec![]),
        AccessPolicy::review_only(),
    ] {
        policy.extension = Some(Arc::new(Extension(Arc::new(AtomicUsize::new(0)))));
        assert!(ToolExecutor::with_policy(root.path(), &policy).is_err());
    }
}
