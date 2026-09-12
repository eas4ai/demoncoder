use clap::Parser;
use demoncoder::{
    config::{Args, Connection},
    workflow::{
        allocation::{Allocation, Limits},
        runtime::{Identity, Record, SharedRuntime, session_budget::SessionHookAllowance},
        state::Task,
        store::Store,
        workspace::{self, CaptureScope},
    },
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn explicit_session_hook_options_allow_zero_calls() {
    let parsed = Args::try_parse_from([
        "demoncoder",
        "--session-hook-seconds",
        "30",
        "--session-hook-model-calls",
        "0",
        "--session-hook-tool-calls",
        "0",
    ]);
    assert!(
        parsed.is_ok(),
        "explicit session grant rejected: {}",
        parsed.err().unwrap()
    );
    let mut zero = SessionHookAllowance::new(Limits {
        seconds: 30,
        model_calls: 0,
        tool_calls: 0,
    })
    .unwrap();
    assert!(zero.allocation.admit(true).is_err());
    assert!(zero.allocation.admit(false).is_err());
    assert_eq!(
        (zero.allocation.model_calls, zero.allocation.tool_calls),
        (0, 0)
    );
}

#[test]
fn session_grant_is_a_separate_durable_record_field() {
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let allocation = Allocation::new(Limits::default()).unwrap();
    let payload = json!({
        "workspace":"/workspace", "identity":Identity::from(&connection),
        "archived":[], "next_task":1, "checkpoint_cursor":0, "operations":[],
        "messages":[], "recovery_pending":false, "decisions":[],
        "session_hook_allowance":{"allocation":allocation}
    });
    let decoded = serde_json::from_value::<Record>(payload.clone());
    assert!(
        decoded.is_ok(),
        "durable session grant rejected: {}",
        decoded.err().unwrap()
    );
    let decoded: Record = serde_json::from_value(payload.clone()).unwrap();
    assert!(decoded.task.is_none() && decoded.allocation.is_none());
    assert_eq!(
        serde_json::to_value(decoded).unwrap()["session_hook_allowance"],
        payload["session_hook_allowance"]
    );
}

#[test]
fn session_options_require_all_three_and_validate_programmatic_values() {
    assert!(
        Args::try_parse_from(["demoncoder"])
            .unwrap()
            .session_hook_limits()
            .unwrap()
            .is_none()
    );
    for mask in 1..7 {
        let mut argv = vec!["demoncoder"];
        for (bit, option) in [
            (1, "--session-hook-seconds"),
            (2, "--session-hook-model-calls"),
            (4, "--session-hook-tool-calls"),
        ] {
            if mask & bit != 0 {
                argv.extend([option, "1"]);
            }
        }
        assert!(
            Args::try_parse_from(argv).is_err(),
            "partial grant accepted: {mask}"
        );
        let mut args = Args::try_parse_from(["demoncoder"]).unwrap();
        args.session_hook_seconds = (mask & 1 != 0).then_some(1);
        args.session_hook_model_calls = (mask & 2 != 0).then_some(1);
        args.session_hook_tool_calls = (mask & 4 != 0).then_some(1);
        assert!(
            args.session_hook_limits().is_err(),
            "programmatic partial grant accepted: {mask}"
        );
    }
    for (seconds, models, tools, valid) in [
        (1, 0, 0, true),
        (86400, 4096, 4096, true),
        (0, 1, 1, false),
        (86401, 1, 1, false),
        (1, 4097, 1, false),
        (1, 1, 4097, false),
    ] {
        let mut args = Args::try_parse_from(["demoncoder"]).unwrap();
        args.session_hook_seconds = Some(seconds);
        args.session_hook_model_calls = Some(models);
        args.session_hook_tool_calls = Some(tools);
        assert_eq!(args.session_hook_limits().is_ok(), valid);
        let argv = [
            "demoncoder".to_owned(),
            "--session-hook-seconds".into(),
            seconds.to_string(),
            "--session-hook-model-calls".into(),
            models.to_string(),
            "--session-hook-tool-calls".into(),
            tools.to_string(),
        ];
        assert_eq!(Args::try_parse_from(argv).is_ok(), valid);
        assert_eq!(
            SessionHookAllowance::new(Limits {
                seconds,
                model_calls: models,
                tool_calls: tools
            })
            .is_ok(),
            valid
        );
    }
    for limits in [
        Limits {
            seconds: 1,
            model_calls: 0,
            tool_calls: 1,
        },
        Limits {
            seconds: 1,
            model_calls: 1,
            tool_calls: 0,
        },
    ] {
        assert!(
            Allocation::new(limits).is_err(),
            "session zero-call policy leaked into task validation"
        );
    }
}

// HOME belongs to the child process only. Other tests never see a changed environment.
fn isolated(name: &str, check: impl FnOnce()) {
    if std::env::var("DEMONCODER_SESSION_ALLOWANCE_CASE").as_deref() == Ok(name) {
        check();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("HOME", root.path())
        .env("DEMONCODER_SESSION_ALLOWANCE_CASE", name)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn connection() -> Connection {
    serde_json::from_value(json!({"adapter":"openai-api", "model":"fixture"})).unwrap()
}

fn grant() -> Limits {
    Limits {
        seconds: 60,
        model_calls: 2,
        tool_calls: 3,
    }
}

fn open(
    root: &Path,
    resume: Option<&Path>,
    limits: Option<&Limits>,
) -> anyhow::Result<(SharedRuntime, bool)> {
    SharedRuntime::open_with_session_hooks(
        root,
        &connection(),
        resume,
        &CaptureScope::default(),
        limits,
    )
}

// Trusted storage fixture updates prove ledger persistence only. Production runner
// admission and spending are intentionally outside this prerequisite's coverage.
fn seed_consumed(directory: &Path) -> Value {
    let mut store = Store::open(directory).unwrap();
    let mut value = store.read().unwrap();
    let allocation = &mut value["session_hook_allowance"]["allocation"];
    allocation["model_calls"] = json!(1);
    allocation["tool_calls"] = json!(2);
    allocation["usage"] = json!({
        "reported_input":7, "reported_output":13, "reported_cached":0, "reported_cost_usd":0.25,
        "unknown_input":false, "unknown_output":true, "unknown_cached":true, "unknown_cost":true
    });
    value["phase"] = json!("worker");
    store.write(&value).unwrap();
    value
}

fn assert_preserved(before: &Value, after: &Value) {
    let before = &before["session_hook_allowance"]["allocation"];
    let after = &after["session_hook_allowance"]["allocation"];
    assert!(before.is_object() && after.is_object());
    for field in [
        "limits",
        "started_ms",
        "deadline_ms",
        "model_calls",
        "tool_calls",
        "usage",
        "clock_invalid",
    ] {
        assert_eq!(
            before[field], after[field],
            "session allowance changed: {field}"
        );
    }
    assert!(after["observed_ms"].as_u64().unwrap() >= before["observed_ms"].as_u64().unwrap());
}

#[test]
fn disk_resume_preserves_grant_and_rejects_changes_before_writing() {
    isolated(
        "disk_resume_preserves_grant_and_rejects_changes_before_writing",
        || {
            let root = tempfile::tempdir().unwrap();
            let (runtime, resumed) = open(root.path(), None, Some(&grant())).unwrap();
            assert!(!resumed);
            assert!(runtime.record().unwrap().allocation.is_none());
            let directory = runtime.directory().unwrap();
            drop(runtime);
            let before = seed_consumed(&directory);
            let bytes = std::fs::read(directory.join("state.json")).unwrap();
            for changed in [
                None,
                Some(Limits {
                    seconds: 61,
                    ..grant()
                }),
                Some(Limits {
                    model_calls: 3,
                    ..grant()
                }),
                Some(Limits {
                    tool_calls: 4,
                    ..grant()
                }),
            ] {
                let error = open(root.path(), Some(&directory), changed.as_ref())
                    .err()
                    .unwrap()
                    .to_string();
                assert!(
                    error.contains("original")
                        && error.contains("--session-hook")
                        && error.contains("new session"),
                    "{error}"
                );
                assert_eq!(std::fs::read(directory.join("state.json")).unwrap(), bytes);
            }
            let (runtime, resumed) = open(root.path(), Some(&directory), Some(&grant())).unwrap();
            assert!(resumed);
            assert!(runtime.record().unwrap().recovery_pending);
            assert_preserved(
                &before,
                &serde_json::to_value(runtime.record().unwrap()).unwrap(),
            );
            assert!(
                runtime.begin_model("worker").is_err(),
                "recovery hold was bypassed"
            );
            drop(runtime);
            assert_preserved(&before, &Store::open(&directory).unwrap().read().unwrap());
        },
    );
}

#[test]
fn old_records_cannot_gain_session_funding_during_resume() {
    isolated(
        "old_records_cannot_gain_session_funding_during_resume",
        || {
            let root = tempfile::tempdir().unwrap();
            let (runtime, _) = SharedRuntime::open(root.path(), &connection(), None).unwrap();
            let directory = runtime.directory().unwrap();
            assert!(runtime.record().unwrap().session_hook_allowance.is_none());
            drop(runtime);
            let mut store = Store::open(&directory).unwrap();
            let mut value = store.read().unwrap();
            value
                .as_object_mut()
                .unwrap()
                .remove("session_hook_allowance");
            store.write(&value).unwrap();
            drop(store);
            let bytes = std::fs::read(directory.join("state.json")).unwrap();
            assert!(open(root.path(), Some(&directory), Some(&grant())).is_err());
            assert_eq!(std::fs::read(directory.join("state.json")).unwrap(), bytes);
            let (runtime, resumed) = SharedRuntime::open_with_scope(
                root.path(),
                &connection(),
                Some(&directory),
                &CaptureScope::default(),
            )
            .unwrap();
            assert!(resumed);
            assert!(runtime.record().unwrap().session_hook_allowance.is_none());
        },
    );
}

#[test]
fn task_allocation_archive_and_reconcile_do_not_replenish_session_grant() {
    isolated(
        "task_allocation_archive_and_reconcile_do_not_replenish_session_grant",
        || {
            let root = tempfile::tempdir().unwrap();
            let (runtime, _) = open(root.path(), None, Some(&grant())).unwrap();
            let directory = runtime.directory().unwrap();
            drop(runtime);
            let before = seed_consumed(&directory);
            let (runtime, _) = open(root.path(), Some(&directory), Some(&grant())).unwrap();
            runtime
                .reconcile("fixture inspected the interrupted phase", None)
                .unwrap();
            for id in 1..=2 {
                runtime.allocate(Limits::default(), None).unwrap();
                let task = Task::new(
                    id,
                    "fixture task".into(),
                    vec![],
                    workspace::capture(root.path()).unwrap(),
                    1,
                )
                .unwrap();
                runtime.save_task(&Some(task), id + 1, None).unwrap();
                runtime.begin_phase("worker", None).unwrap();
                let model = runtime.begin_model("worker").unwrap();
                runtime.finish_model(model).unwrap();
                runtime.finish_phase().unwrap();
                runtime
                    .reconcile("fixture inspected missing usage", None)
                    .unwrap();
                runtime.archive().unwrap();
                let record = runtime.record().unwrap();
                assert!(record.allocation.is_none());
                assert_eq!(record.archived.len(), id as usize);
                assert_eq!(
                    record
                        .archived
                        .last()
                        .unwrap()
                        .allocation
                        .as_ref()
                        .unwrap()
                        .model_calls,
                    1
                );
                assert_preserved(&before, &serde_json::to_value(record).unwrap());
            }
            drop(runtime);
            assert_preserved(&before, &Store::open(&directory).unwrap().read().unwrap());
        },
    );
}

#[test]
fn serialized_expiry_and_clock_rollback_survive_resume_and_updates() {
    isolated(
        "serialized_expiry_and_clock_rollback_survive_resume_and_updates",
        || {
            let root = tempfile::tempdir().unwrap();
            let (runtime, _) = open(root.path(), None, Some(&grant())).unwrap();
            let directory = runtime.directory().unwrap();
            drop(runtime);
            let mut store = Store::open(&directory).unwrap();
            let mut value = store.read().unwrap();
            value["session_hook_allowance"]["allocation"]["deadline_ms"] = json!(1);
            store.write(&value).unwrap();
            drop(store);
            let (runtime, _) = open(root.path(), Some(&directory), Some(&grant())).unwrap();
            assert_eq!(
                runtime
                    .record()
                    .unwrap()
                    .session_hook_allowance
                    .unwrap()
                    .allocation
                    .remaining_ms()
                    .unwrap(),
                0
            );
            runtime
                .reconcile("expired session stays expired", None)
                .unwrap();
            assert_eq!(
                runtime
                    .record()
                    .unwrap()
                    .session_hook_allowance
                    .unwrap()
                    .allocation
                    .deadline_ms,
                1
            );
            drop(runtime);
            let mut store = Store::open(&directory).unwrap();
            let mut value = store.read().unwrap();
            value["session_hook_allowance"]["allocation"]["observed_ms"] =
                json!(demoncoder::workflow::allocation::now_ms().unwrap() + 60_000);
            store.write(&value).unwrap();
            drop(store);
            let (runtime, _) = open(root.path(), Some(&directory), Some(&grant())).unwrap();
            let allocation = runtime
                .record()
                .unwrap()
                .session_hook_allowance
                .unwrap()
                .allocation;
            assert!(allocation.clock_invalid && allocation.remaining_ms().is_err());
            runtime.hold().unwrap();
            drop(runtime);
            let (runtime, _) = open(root.path(), Some(&directory), Some(&grant())).unwrap();
            assert!(
                runtime
                    .record()
                    .unwrap()
                    .session_hook_allowance
                    .unwrap()
                    .allocation
                    .clock_invalid
            );
        },
    );
}

#[test]
fn runtime_rejects_invalid_programmatic_limits_before_creating_a_session() {
    isolated(
        "runtime_rejects_invalid_programmatic_limits_before_creating_a_session",
        || {
            let root = tempfile::tempdir().unwrap();
            for limits in [
                Limits {
                    seconds: 0,
                    ..grant()
                },
                Limits {
                    seconds: 86401,
                    ..grant()
                },
                Limits {
                    model_calls: 4097,
                    ..grant()
                },
                Limits {
                    tool_calls: 4097,
                    ..grant()
                },
            ] {
                assert!(open(root.path(), None, Some(&limits)).is_err());
            }
            let private = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".demoncoder");
            assert!(
                !private.exists(),
                "invalid configuration created session state"
            );
        },
    );
}

#[test]
fn concurrent_session_opens_reserve_distinct_private_directories() {
    isolated(
        "concurrent_session_opens_reserve_distinct_private_directories",
        || {
            let root = tempfile::tempdir().unwrap();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(32));
            let workers: Vec<_> = (0..32)
                .map(|_| {
                    let barrier = barrier.clone();
                    let workspace = root.path().to_owned();
                    std::thread::spawn(move || {
                        barrier.wait();
                        open(&workspace, None, Some(&grant()))
                    })
                })
                .collect();
            let mut runtimes = Vec::new();
            let mut failures = Vec::new();
            for worker in workers {
                match worker.join().unwrap() {
                    Ok((runtime, resumed)) => {
                        assert!(!resumed);
                        runtimes.push(runtime);
                    }
                    Err(error) => failures.push(format!("{error:#}")),
                }
            }
            assert!(
                failures.is_empty(),
                "legitimate concurrent session opens failed: {failures:?}"
            );
            let directories: std::collections::BTreeSet<_> = runtimes
                .iter()
                .map(|runtime| runtime.directory().unwrap())
                .collect();
            assert_eq!(directories.len(), 32);
            for runtime in runtimes {
                let directory = runtime.directory().unwrap();
                assert!(
                    Store::open(&directory).is_err(),
                    "concurrent opens lost exclusive ownership"
                );
                drop(runtime);
                let restored: Record =
                    serde_json::from_value(Store::open(&directory).unwrap().read().unwrap())
                        .unwrap();
                assert_eq!(
                    restored.session_hook_allowance.unwrap().allocation.limits,
                    grant()
                );
            }
        },
    );
}

#[test]
fn actual_main_persists_options_and_rejects_mismatched_resume_before_adapter_start() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let config = home.path().join("settings.toml");
    std::fs::write(&config, "default_connection = 'fixture'\n[connections.fixture]\nadapter = 'openai-api'\nmodel = 'fixture'\n").unwrap();
    let launch = |resume: Option<&Path>, seconds: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_demoncoder"));
        command
            .env("HOME", home.path())
            .env_remove("OPENAI_API_KEY")
            .args(["--config"])
            .arg(&config)
            .arg("--workspace")
            .arg(workspace.path())
            .args([
                "--trust-workspace",
                "--session-hook-seconds",
                seconds,
                "--session-hook-model-calls",
                "0",
                "--session-hook-tool-calls",
                "0",
            ]);
        if let Some(path) = resume {
            command.arg("--resume").arg(path);
        }
        command.output().unwrap()
    };
    let output = launch(None, "30");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("OPENAI_API_KEY"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sessions: Vec<_> = std::fs::read_dir(home.path().join(".demoncoder/sessions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(sessions.len(), 1);
    let directory = &sessions[0];
    let state = Store::open(directory).unwrap().read().unwrap();
    assert_eq!(
        state["session_hook_allowance"]["allocation"]["limits"],
        json!({"seconds":30,"model_calls":0,"tool_calls":0})
    );
    assert!(state["allocation"].is_null() && state["task"].is_null());
    let bytes = std::fs::read(directory.join("state.json")).unwrap();
    let output = launch(Some(directory), "31");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("original") && diagnostic.contains("--session-hook"),
        "{diagnostic}"
    );
    assert!(
        !diagnostic.contains("OPENAI_API_KEY"),
        "adapter startup ran before rejecting a mismatched grant"
    );
    assert_eq!(std::fs::read(directory.join("state.json")).unwrap(), bytes);
    assert!(
        Store::open(directory).unwrap().read().unwrap()["operations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
