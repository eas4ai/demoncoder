use super::*;
use crate::plugins::observer::{ObserverConfig, Status};
struct Later {
    release: Arc<tokio::sync::Notify>,
    effect: std::path::PathBuf,
    event: HookEvent,
}
#[async_trait::async_trait]
impl HookRunner for Later {
    fn observer_config(&self) -> Option<ObserverConfig> {
        Some(ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        })
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.release.notified().await;
        std::fs::write(&self.effect, "actual late effect")?;
        Ok(RawOutcome::Callback {
            value: output(self.event),
        })
    }
}
fn observer(case: &DispatchCase, release: Arc<tokio::sync::Notify>, once: bool) -> Registration {
    let mut declaration = case.declaration(once);
    declaration.class = HandlerClass::Observer;
    declaration.required_gate = false;
    Registration {
        declaration,
        runner: Arc::new(Later {
            release,
            effect: case.root.path().join("late"),
            event: case.event,
        }),
        revalidation: None,
    }
}
fn transferred_hook(case: &DispatchCase) -> crate::plugins::receipts::HookReceipt {
    super::super::super::hooks(&case.runtime.record().unwrap())
        .find(|h| h.observer.is_some())
        .unwrap()
        .clone()
}
#[tokio::test]
async fn observer_transfer_releases_both_dispatchers_before_actual_once_completion() {
    for event in [HookEvent::PreToolUse, HookEvent::PostToolUse] {
        let case = DispatchCase::new(event);
        let release = Arc::new(tokio::sync::Notify::new());
        case.dispatch(0, observer(&case, release.clone(), true))
            .await
            .unwrap();
        let hook = transferred_hook(&case);
        assert!(hook.outcome.is_none());
        assert_ne!(
            hook.once.unwrap().state,
            crate::plugins::once::OnceState::Succeeded
        );
        assert!(!case.root.path().join("late").exists());
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            while transferred_hook(&case).outcome.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let hook = transferred_hook(&case);
        assert_eq!(hook.observer.unwrap().status, Status::Completed);
        assert_eq!(
            hook.once.unwrap().state,
            crate::plugins::once::OnceState::Succeeded
        );
        assert_eq!(
            std::fs::read_to_string(case.root.path().join("late")).unwrap(),
            "actual late effect"
        );
        case.verify_originals();
    }
}
#[tokio::test]
async fn explicit_cancel_stops_transferred_observer_before_late_effect() {
    let case = DispatchCase::new(HookEvent::PostToolUse);
    let release = Arc::new(tokio::sync::Notify::new());
    case.dispatch(0, observer(&case, release.clone(), true))
        .await
        .unwrap();
    case.runtime.stop_observers(None, false).await.unwrap();
    release.notify_one();
    assert!(!case.root.path().join("late").exists());
    let hook = transferred_hook(&case);
    assert_eq!(hook.observer.unwrap().status, Status::Interrupted);
    assert_eq!(
        hook.once.unwrap().state,
        crate::plugins::once::OnceState::Unknown
    );
    case.verify_originals();
}

#[tokio::test]
async fn legacy_required_policy_preserves_observer_combined_gate_and_once_evidence() {
    let case = DispatchCase::new(HookEvent::PostToolUse);
    let release = Arc::new(tokio::sync::Notify::new());
    case.dispatch(0, observer(&case, release, true))
        .await
        .unwrap();
    let original = serde_json::to_value(transferred_hook(&case)).unwrap();
    for (class, required) in [
        ("observer", false),
        ("combined", true),
        ("decision_gate", true),
    ] {
        for state in ["succeeded", "unknown"] {
            let mut legacy = original.clone();
            legacy.as_object_mut().unwrap().remove("required_gate");
            legacy.as_object_mut().unwrap().remove("observer");
            legacy["class"] = class.into();
            legacy["once"]["state"] = state.into();
            let restored: crate::plugins::receipts::HookReceipt =
                serde_json::from_value(legacy).unwrap();
            assert_eq!(restored.required_gate, required);
            assert_eq!(serde_json::to_value(restored.once).unwrap()["state"], state);
        }
    }
    case.runtime.stop_observers(None, false).await.unwrap();
}

#[tokio::test]
async fn weak_observer_job_cannot_keep_runtime_or_write_after_owner_loss() {
    let mut case = DispatchCase::new(HookEvent::PostToolUse);
    let release = Arc::new(tokio::sync::Notify::new());
    case.dispatch(0, observer(&case, release.clone(), true))
        .await
        .unwrap();
    let weak = case.runtime.downgrade();
    case.calls.clear();
    drop(case.runtime);
    assert!(weak.upgrade().is_err());
    release.notify_one();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!case.root.path().join("late").exists());
}

#[tokio::test]
async fn observer_capacity_is_reserved_before_queueing_and_owner_replacement_is_fenced() {
    let case = DispatchCase::new(HookEvent::PostToolUse);
    let release = Arc::new(tokio::sync::Notify::new());
    let registrations = (0..9)
        .map(|index| {
            let mut registration = observer(&case, release.clone(), false);
            registration.declaration.identity.index = index;
            registration.declaration.identity.declaration = format!("observer-{index}");
            registration
        })
        .collect();
    let error = case
        .dispatch_registrations(0, registrations)
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("capacity exhausted"));
    assert!(case.runtime.allocate(Default::default(), None).is_err());
    case.runtime.stop_observers(None, false).await.unwrap();
    release.notify_waiters();
    assert!(!case.root.path().join("late").exists());
}

#[tokio::test]
async fn observer_restart_retains_unknown_transfer_without_replaying() {
    let mut case = DispatchCase::new(HookEvent::PostToolUse);
    let release = Arc::new(tokio::sync::Notify::new());
    case.dispatch(0, observer(&case, release.clone(), true))
        .await
        .unwrap();
    case.calls.clear();
    case.runtime = super::super::reopen(case.runtime);
    release.notify_one();
    assert!(case.runtime.record().unwrap().recovery_pending);
    let hook = transferred_hook(&case);
    assert!(hook.outcome.is_none());
    assert_eq!(hook.observer.unwrap().status, Status::Interrupted);
    assert_eq!(case.runtime.unresolved_plugin_once().unwrap().len(), 1);
    assert!(!case.root.path().join("late").exists());
}

#[tokio::test]
async fn observer_delivery_reservation_survives_restart_without_duplicate_context() {
    let mut case = DispatchCase::new(HookEvent::PostToolUse);
    let release = Arc::new(tokio::sync::Notify::new());
    case.dispatch(0, observer(&case, release.clone(), true))
        .await
        .unwrap();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while transferred_hook(&case).outcome.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let delivery = case
        .runtime
        .reserve_observer_context("worker", None, false)
        .unwrap()
        .unwrap();
    assert!(delivery.text.contains("prepared"));
    case.calls.clear();
    case.runtime = super::super::reopen(case.runtime);
    assert_eq!(
        transferred_hook(&case).observer.unwrap().delivery,
        crate::plugins::observer::Delivery::Withheld
    );
    assert!(
        case.runtime
            .reserve_observer_context("worker", None, false)
            .unwrap()
            .is_none()
    );
    assert!(case.runtime.complete_observer_context(&delivery).is_err());
}

struct Immediate {
    effects: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl HookRunner for Immediate {
    fn observer_config(&self) -> Option<ObserverConfig> {
        Some(ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        })
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(RawOutcome::Callback {
            value: output(HookEvent::PostToolUse),
        })
    }
}
#[tokio::test]
async fn observer_pending_context_queue_rejects_effect_65_before_launch() {
    let case = DispatchCase::with_count(HookEvent::PostToolUse, 65);
    let effects = Arc::new(AtomicUsize::new(0));
    for index in 0..65 {
        let mut registration = observer(&case, Arc::new(tokio::sync::Notify::new()), false);
        registration.runner = Arc::new(Immediate {
            effects: effects.clone(),
        });
        let result = case.dispatch(index, registration).await;
        if index == 64 {
            assert!(
                format!("{:#}", result.unwrap_err())
                    .contains("pending delivery capacity exhausted")
            );
            break;
        }
        result.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let done = {
                    let runtime = case.runtime.0.lock().unwrap();
                    runtime.observers.stopped()
                };
                if done {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(effects.load(Ordering::SeqCst), 64);
    case.runtime.stop_observers(None, false).await.unwrap();
}
