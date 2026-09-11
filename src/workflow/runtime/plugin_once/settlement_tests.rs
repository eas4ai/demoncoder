use super::*;
use std::{future::Future, pin::Pin, sync::atomic::AtomicBool};

struct Paused {
    entered: Arc<AtomicBool>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl HookRunner for Paused {
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.entered.store(true, Ordering::SeqCst);
        self.release.notified().await;
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}
fn registrations(
    case: &DispatchCase,
) -> (Vec<Registration>, Arc<AtomicBool>, Arc<tokio::sync::Notify>) {
    let entered = Arc::new(AtomicBool::new(false));
    let release = Arc::new(tokio::sync::Notify::new());
    let first = Registration {
        declaration: case.declaration(true),
        runner: Arc::new(Prepared {
            prepares: Arc::new(AtomicUsize::new(0)),
            runs: Arc::new(AtomicUsize::new(0)),
            unknown: false,
        }),
        revalidation: None,
    };
    let mut later = case.declaration(false);
    later.identity.declaration = "later-group".into();
    later.identity.index = 1;
    let later = Registration {
        declaration: later,
        runner: Arc::new(Paused {
            entered: entered.clone(),
            release: release.clone(),
        }),
        revalidation: None,
    };
    (vec![first, later], entered, release)
}
async fn wait_for_later(pending: Pin<&mut impl Future<Output = Result<()>>>, entered: &AtomicBool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            result = pending => panic!("later group did not pause: {result:?}"),
            _ = async { while !entered.load(Ordering::SeqCst) { tokio::task::yield_now().await; } } => {}
        }
    }).await.unwrap();
}
#[tokio::test]
async fn failed_attestation_rejects_while_later_post_group_owns_unsettled_context() {
    let case = DispatchCase::new(HookEvent::PostToolUse);
    let (registrations, entered, release) = registrations(&case);
    let pending = case.dispatch_registrations(0, registrations);
    tokio::pin!(pending);
    wait_for_later(pending.as_mut(), &entered).await;
    let target = case
        .runtime
        .unresolved_plugin_once()
        .unwrap()
        .pop()
        .unwrap();
    let attestation = case.runtime.reconcile_failed_plugin_once(
        &target,
        "developer",
        "must not reconcile while proposal owner is active",
    );
    release.notify_one();
    pending.await.unwrap();
    assert!(
        attestation.is_err(),
        "failure attestation was accepted while lifecycle settlement was still live"
    );
    let record = case.runtime.record().unwrap();
    let hook = hooks(&record).find(|h| h.once.is_some()).unwrap();
    let attempt = hook.once.as_ref().unwrap();
    assert_eq!(attempt.state, OnceState::Succeeded);
    assert!(attempt.reconciliation.is_none());
    let lifecycle = record
        .operations
        .iter()
        .find_map(|o| o.tool_receipt.as_ref()?.plugin_lifecycle.as_ref())
        .unwrap();
    assert!(lifecycle.settled);
    assert_eq!(lifecycle.messages.len(), 1);
    assert_eq!(lifecycle.messages[0].text, "prepared");
    assert!(case.runtime.unresolved_plugin_once().unwrap().is_empty());
    case.verify_originals();
}
#[tokio::test]
async fn abandoned_post_owner_allows_exact_failed_attestation_before_and_after_reopen() {
    for restart in [false, true] {
        let mut case = DispatchCase::new(HookEvent::PostToolUse);
        let target;
        {
            let (registrations, entered, _release) = registrations(&case);
            let pending = case.dispatch_registrations(0, registrations);
            tokio::pin!(pending);
            wait_for_later(pending.as_mut(), &entered).await;
            target = case
                .runtime
                .unresolved_plugin_once()
                .unwrap()
                .pop()
                .unwrap();
        }
        case.verify_originals();
        let prior = serde_json::to_value(
            hooks(&case.runtime.record().unwrap())
                .find(|h| h.once.is_some())
                .unwrap()
                .outcome
                .as_ref(),
        )
        .unwrap();
        if restart {
            // Release every EventSink runtime handle before reopening the persisted record.
            case.calls.clear();
            case.runtime = reopen(case.runtime);
        }
        case.runtime
            .reconcile_failed_plugin_once(
                &target,
                "developer",
                "post lifecycle was abandoned before applying context",
            )
            .unwrap();
        let record = case.runtime.record().unwrap();
        let hook = hooks(&record).find(|h| h.once.is_some()).unwrap();
        assert_eq!(serde_json::to_value(hook.outcome.as_ref()).unwrap(), prior);
        let attempt = hook.once.as_ref().unwrap();
        assert_eq!(attempt.state, OnceState::Unknown);
        assert!(attempt.reconciliation.is_some());
        let lifecycle = record
            .operations
            .iter()
            .find_map(|o| o.tool_receipt.as_ref()?.plugin_lifecycle.as_ref())
            .unwrap();
        assert!(!lifecycle.settled);
        assert!(lifecycle.messages.is_empty());
    }
}
