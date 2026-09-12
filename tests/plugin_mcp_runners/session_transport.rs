use super::*;
use demoncoder::plugins::services::ServiceState;
#[path = "../support/session_transport.rs"]
mod host;

fn peer() -> Peer {
    Peer::new(|headers, request| {
        if headers.starts_with("DELETE") || request.get("id").is_none() {
            return (202, String::new(), vec![]);
        }
        let result = match request["method"].as_str().unwrap() {
            "initialize" => init(),
            "tools/list" => json!({"tools":[metadata()]}),
            "tools/call" => json!({"content":[],"structuredContent":{}}),
            other => panic!("unexpected request {other}"),
        };
        let (status, mut headers, body) = rpc(request, result);
        if request["method"] == "initialize" {
            headers.push_str("Mcp-Session-Id: native-fixture\r\n");
        }
        (status, headers, body)
    })
}
fn managed(
    host: &host::Host,
    package: Arc<plugins::Package>,
    transport: ServiceTransport,
    max_calls: u32,
) -> Arc<ManagedService> {
    use std::os::unix::fs::MetadataExt;
    let root = host.root.path().metadata().unwrap();
    ManagedServices::default()
        .admit(
            package,
            ServiceConfig {
                identity: ServiceIdentity {
                    workspace: (root.dev(), root.ino()),
                    role: "worker".into(),
                    generation: "fixture-generation".into(),
                    state: "state-1".into(),
                    credential_revision: "credentials-1".into(),
                },
                transport,
                tools: vec![AdmittedTool {
                    metadata: metadata(),
                    read_only: true,
                }],
                timeout_ms: 2000,
                max_calls,
            },
        )
        .unwrap()
}
fn plans(
    package: Arc<plugins::Package>,
    service: Arc<ManagedService>,
) -> Vec<(HookEvent, Vec<Registration>)> {
    [HookEvent::SessionStart, HookEvent::SessionEnd]
        .into_iter()
        .map(|event| {
            let mut d = declaration("session-mcp", HookDialect::Native, HandlerClass::Observer);
            d.matcher = Matcher::default();
            (
                event,
                vec![
                    McpRunner::registration_for_event(
                        package.clone(),
                        d,
                        event,
                        McpBinding {
                            service: service.clone(),
                            tool: "gate".into(),
                            input: json!({"event":"${hook_event_name}"}),
                        },
                        None,
                        McpConfig::default(),
                    )
                    .unwrap(),
                ],
            )
        })
        .collect()
}
#[tokio::test]
async fn native_session_mcp_http_no_prompt_retains_connection_and_call_cap() {
    let _lock = FIXTURES.lock().await;
    for max_calls in [1, 2] {
        let peer = peer();
        let mut host = host::Host::new(true);
        let package = package(HookDialect::Native);
        let service = managed(
            &host,
            package.clone(),
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
            max_calls,
        );
        let running = host.start(plans(package, service.clone())).await;
        assert_eq!(
            service.state(),
            ServiceState::Ready,
            "native startup service did not initialize"
        );
        running.stop().await;
        assert_eq!(
            peer.methods().iter().filter(|m| *m == "initialize").count(),
            1
        );
        assert_eq!(
            peer.methods().iter().filter(|m| *m == "tools/call").count(),
            max_calls as usize
        );
        assert!(
            peer.requests
                .lock()
                .unwrap()
                .iter()
                .any(|(h, _)| h.starts_with("DELETE")),
            "host returned before MCP session cleanup"
        );
        assert!(matches!(
            service.state(),
            ServiceState::Stopped | ServiceState::Failed
        ));
        assert!(host.runtime.record().unwrap().allocation.is_none());
        host.assert_unspent();
    }
}

fn stdio_service(host: &host::Host, token: &str) -> (Arc<plugins::Package>, Arc<ManagedService>) {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"mcp-session","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        source.path().join("peer.py"),
        include_str!("../fixtures/plugins/mcp_session_peer.py"),
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    let transport = ServiceTransport::Stdio(
        demoncoder::plugins::runners::CommandConfig::new(
            demoncoder::plugins::runners::CommandProgram::Argv(vec![
                "/usr/bin/python3".into(),
                "${CODEX_PLUGIN_ROOT}/peer.py".into(),
                token.into(),
            ]),
        )
        .into(),
    );
    let service = managed(host, package.clone(), transport, 2);
    (package, service)
}
fn processes(token: &str) -> Vec<u32> {
    descendants(std::process::id())
        .into_iter()
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(token))
        })
        .collect()
}

#[tokio::test]
async fn native_session_mcp_changed_view_between_discovery_and_call_sends_no_tool_effect() {
    let _lock = FIXTURES.lock().await;
    let mut host = host::Host::new(true);
    let watched = host.root.path().join("watched");
    std::fs::write(&watched, "original").unwrap();
    let peer = Peer::new(move |headers, request| {
        if headers.starts_with("DELETE") || request.get("id").is_none() {
            return (202, String::new(), vec![]);
        }
        let result = match request["method"].as_str().unwrap() {
            "initialize" => init(),
            "tools/list" => {
                std::fs::write(&watched, "replaced").unwrap();
                json!({"tools":[metadata()]})
            }
            "tools/call" => json!({"content":[],"structuredContent":{}}),
            other => panic!("unexpected {other}"),
        };
        rpc(request, result)
    });
    let package = package(HookDialect::Native);
    let service = managed(
        &host,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
        2,
    );
    let mut plans = plans(package, service);
    plans.truncate(1);
    plans[0].1[0].declaration.reads =
        GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap();
    host.start(plans).await.stop().await;
    assert!(
        peer.methods().contains(&"tools/list".into()),
        "discovery boundary was not reached"
    );
    assert!(
        !peer.methods().contains(&"tools/call".into()),
        "changed retained view released a tool effect"
    );
}
#[tokio::test]
async fn native_session_mcp_stdio_reuses_view_and_reaps_descendants_with_retained_handles() {
    let _lock = FIXTURES.lock().await;
    for abort in [false, true] {
        let mut host = host::Host::new(true);
        let token = format!("native-session-cleanup-{}-{abort}", std::process::id());
        let (package, service) = stdio_service(&host, &token);
        let mut running = host.start(plans(package, service.clone())).await;
        let pids = processes(&token);
        assert!(
            pids.len() >= 2,
            "service and detached descendant did not execute: {pids:?}"
        );
        assert_eq!(service.state(), ServiceState::Ready);
        if abort {
            running.owner.abort();
            assert!(matches!((&mut running.owner).await, Err(error) if error.is_cancelled()));
            tokio::time::timeout(Duration::from_secs(3), async {
                while pids
                    .iter()
                    .any(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("aborted host retained service processes");
        } else {
            running.stop().await;
            assert!(
                pids.iter()
                    .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists()),
                "native shutdown returned before process teardown"
            );
            let record = host.runtime.record().unwrap();
            let hooks: Vec<_> = record
                .operations
                .iter()
                .filter_map(|o| match &o.host_invocation {
                    Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(r)) => Some(r),
                    _ => None,
                })
                .flat_map(|r| &r.hooks)
                .collect();
            assert_eq!(hooks.len(), 2);
            assert!(
                hooks
                    .iter()
                    .all(|h| matches!(h.outcome, Some(RawOutcome::Mcp { .. }))),
                "stdio did not execute native end with shared call count: {hooks:?}"
            );
        }
        assert!(host.runtime.record().unwrap().allocation.is_none());
        host.assert_unspent();
        service.stop().await.unwrap();
    }
}

#[tokio::test]
async fn native_session_mcp_survives_independent_task_replace_stop_accept_and_archive() {
    let _lock = FIXTURES.lock().await;
    for stdio in [false, true] {
        let mut host = host::Host::new(true);
        let peer = peer();
        let (package, service) = if stdio {
            stdio_service(&host, "native-session-independent")
        } else {
            let package = package(HookDialect::Native);
            let service = managed(
                &host,
                package.clone(),
                ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
                2,
            );
            (package, service)
        };
        let running = host.start(plans(package, service.clone())).await;
        let deadline = host
            .runtime
            .record()
            .unwrap()
            .session_hook_allowance
            .unwrap()
            .allocation
            .deadline_ms;
        for id in [1, 2] {
            host.runtime
                .allocate(demoncoder::workflow::allocation::Limits::default(), None)
                .unwrap();
            let mut task = demoncoder::workflow::state::Task::new(
                id,
                "independent".into(),
                vec![],
                demoncoder::workflow::workspace::capture(host.root.path()).unwrap(),
                1,
            )
            .unwrap();
            host.runtime
                .save_task(&Some(task.clone()), id + 1, None)
                .unwrap();
            task.stopped = true;
            task.accepted = Some("accepted-independent".into());
            host.runtime.save_task(&Some(task), id + 1, None).unwrap();
            host.runtime.archive().unwrap();
        }
        running
            .commands
            .send(demoncoder::session::Command::Cancel)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            service.state(),
            ServiceState::Ready,
            "unrelated task cancellation revoked session service"
        );
        running.stop().await;
        let record = host.runtime.record().unwrap();
        assert_eq!(
            record
                .session_hook_allowance
                .as_ref()
                .unwrap()
                .allocation
                .deadline_ms,
            deadline
        );
        assert_eq!(record.archived.len(), 2);
        let hooks: Vec<_> = record
            .operations
            .iter()
            .filter_map(|o| match &o.host_invocation {
                Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(r)) => Some(r),
                _ => None,
            })
            .flat_map(|r| &r.hooks)
            .collect();
        assert_eq!(hooks.len(), 2);
        assert!(
            hooks
                .iter()
                .all(|h| matches!(h.outcome, Some(RawOutcome::Mcp { .. }))),
            "independent task prevented end: {hooks:?}"
        );
        host.assert_unspent();
    }
}

#[tokio::test]
async fn native_session_mcp_changed_stdio_snapshot_requires_readmission_without_reinitialize() {
    let _lock = FIXTURES.lock().await;
    let mut host = host::Host::new(true);
    std::fs::write(host.root.path().join("watched"), "original").unwrap();
    let (package, service) = stdio_service(&host, "native-session-changed-view");
    let mut plans = plans(package, service.clone());
    for (_, registrations) in &mut plans {
        registrations[0].declaration.reads =
            GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap();
    }
    let running = host.start(plans).await;
    let pids = processes("native-session-changed-view");
    assert!(pids.len() >= 2);
    std::fs::write(
        host.root.path().join("watched"),
        "must not enter original view",
    )
    .unwrap();
    running.stop().await;
    let record = host.runtime.record().unwrap();
    assert_eq!(
        record
            .operations
            .iter()
            .filter(|o| matches!(
                o.host_invocation,
                Some(demoncoder::workflow::runtime::HostInvocation::PluginService { .. })
            ))
            .count(),
        1,
        "changed view silently created a replacement service"
    );
    let end = record
        .operations
        .iter()
        .find_map(|o| match &o.host_invocation {
            Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(r))
                if matches!(
                    r.facts.subject.occurrence,
                    plugins::receipts::NonToolOccurrence::SessionEnd { .. }
                ) =>
            {
                Some(r)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        end.hooks
            .iter()
            .all(|h| matches!(h.outcome, Some(RawOutcome::Failure { .. }))),
        "changed view ran native end: {end:?}"
    );
    assert!(
        pids.iter()
            .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists())
    );
}

#[tokio::test]
async fn native_session_mcp_stalled_stdio_start_and_end_reap_actual_processes() {
    let _lock = FIXTURES.lock().await;
    for phase in ["start", "end"] {
        let mut host = host::Host::new(true);
        let token = format!("native-session-stall-{phase}");
        let (package, service) = stdio_service(&host, &token);
        let watch = async {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let pids = processes(&token);
                    if pids.len() >= 2 {
                        break pids;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("stalled service did not start real descendants")
        };
        let (running, pids) = tokio::join!(host.start(plans(package, service.clone())), watch);
        if phase == "start" {
            assert!(
                pids.iter()
                    .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists()),
                "startup readiness preceded owned cleanup"
            );
        }
        let ended = std::time::Instant::now();
        running.stop().await;
        assert!(ended.elapsed() < Duration::from_secs(5));
        assert!(
            pids.iter()
                .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists()),
            "end returned with stalled descendants"
        );
        let record = host.runtime.record().unwrap();
        assert!(
            record.recovery_pending,
            "stalled transport lost unknown effects"
        );
        host.assert_unspent();
        service.stop().await.unwrap();
    }
}

#[tokio::test]
async fn native_session_mcp_late_end_reply_is_withheld_within_whole_shutdown_bound() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let _lock = FIXTURES.lock().await;
    let release = Arc::new(AtomicBool::new(false));
    let released = release.clone();
    let peer = Peer::new(move |headers, request| {
        if headers.starts_with("DELETE") || request.get("id").is_none() {
            return (202, String::new(), vec![]);
        }
        let result = match request["method"].as_str().unwrap() {
            "initialize" => init(),
            "tools/list" => json!({"tools":[metadata()]}),
            "tools/call" => {
                if request["params"]["arguments"]["event"] == "SessionEnd" {
                    let deadline = std::time::Instant::now() + Duration::from_secs(4);
                    while !released.load(Ordering::Acquire) && std::time::Instant::now() < deadline
                    {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
                json!({"content":[],"structuredContent":{}})
            }
            other => panic!("unexpected method {other}"),
        };
        let (status, mut headers, body) = rpc(request, result);
        if request["method"] == "initialize" {
            headers.push_str("Mcp-Session-Id: native-delayed\r\n");
        }
        (status, headers, body)
    });
    let mut host = host::Host::new(true);
    let package = package(HookDialect::Native);
    let service = managed(
        &host,
        package.clone(),
        ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
        2,
    );
    let running = host.start(plans(package, service.clone())).await;
    let ended = std::time::Instant::now();
    running.stop().await;
    let elapsed = ended.elapsed();
    release.store(true, Ordering::Release);
    assert!(elapsed < Duration::from_secs(5));
    assert_eq!(
        peer.methods().iter().filter(|m| *m == "tools/call").count(),
        2
    );
    let record = host.runtime.record().unwrap();
    let end = record
        .operations
        .iter()
        .find_map(|o| match &o.host_invocation {
            Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(r))
                if matches!(
                    r.facts.subject.occurrence,
                    plugins::receipts::NonToolOccurrence::SessionEnd { .. }
                ) =>
            {
                Some(r)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        end.hooks
            .iter()
            .all(|h| !matches!(h.outcome, Some(RawOutcome::Mcp { .. }))),
        "late end reply became a pass"
    );
    assert!(record.recovery_pending);
    service.stop().await.unwrap();
}

async fn idle_stdio_ping(fault: &str) {
    let _lock = FIXTURES.lock().await;
    let mut host = host::Host::new(true);
    let aliases = tempfile::tempdir().unwrap();
    let alias = aliases.path().join("credential");
    let first = host.root.path().join("first-private");
    let second = host.root.path().join("second-private");
    std::fs::write(&first, "fixture-private-one").unwrap();
    std::fs::write(&second, "fixture-private-two").unwrap();
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    host.credentials = if fault == "credential" {
        vec![alias.clone()]
    } else {
        vec![alias.clone(), first, second.clone()]
    };
    let watched = host.root.path().join("watched");
    std::fs::write(&watched, "original").unwrap();
    let token = format!("native-session-idle-ping-{fault}");
    let (package, service) = stdio_service(&host, &token);
    let running = host.start(plans(package, service.clone())).await;
    let pids = processes(&token);
    let peer = *pids
        .iter()
        .find(|pid| {
            std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| {
                exe.file_name()
                    .is_some_and(|name| name.as_encoded_bytes().starts_with(b"python"))
            }) && std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|args| {
                args.split(|byte| *byte == 0)
                    .any(|arg| arg.ends_with(b"/peer.py"))
            })
        })
        .expect("confined peer missing");
    if fault == "view" {
        std::fs::write(&watched, "changed").unwrap();
    } else if fault.starts_with("credential") {
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
    }
    let record = host.runtime.record().unwrap();
    let signalled = rustix::process::kill_process(
        rustix::process::Pid::from_raw(peer as i32).unwrap(),
        rustix::process::Signal::USR1,
    );
    let received = || {
        std::fs::read_to_string(format!("/proc/{peer}/comm"))
            .is_ok_and(|name| name.trim() == "mcp-ping-reply")
    };
    let observed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if received()
                || pids
                    .iter()
                    .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    let replied = received();
    let cleaned = pids
        .iter()
        .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists());
    let idle_state = service.state();
    running.stop().await;
    service.stop().await.unwrap();
    signalled.unwrap();
    observed.expect("idle peer neither received a reply nor completed cleanup");
    assert!(pids.len() >= 2);
    assert!(!record.recovery_pending);
    assert!(
        record
            .session_hook_allowance
            .unwrap()
            .allocation
            .remaining_ms()
            .unwrap()
            > 0
    );
    assert!(record.operations.iter().any(|o| matches!(&o.host_invocation,
        Some(demoncoder::workflow::runtime::HostInvocation::NativeSession(lifetime)) if lifetime.end.is_none())));
    let valid = fault == "valid" || fault == "credential-equivalent";
    assert_eq!(
        replied, valid,
        "idle ping reply crossed revoked {fault} authority"
    );
    assert_eq!(
        cleaned, !valid,
        "idle {fault} cleanup did not finish while host remained live"
    );
    if valid {
        assert_eq!(idle_state, ServiceState::Ready);
    }
    host.assert_unspent();
}
#[tokio::test]
async fn native_session_idle_stdio_ping_with_unchanged_authority_gets_reply() {
    idle_stdio_ping("valid").await;
}
#[tokio::test]
async fn native_session_idle_stdio_ping_after_view_change_gets_no_reply_and_reaps() {
    idle_stdio_ping("view").await;
}
#[tokio::test]
async fn native_session_idle_stdio_ping_after_credential_remap_gets_no_reply_and_reaps() {
    idle_stdio_ping("credential").await;
}
#[tokio::test]
async fn native_session_idle_stdio_ping_with_equivalent_protection_gets_reply() {
    idle_stdio_ping("credential-equivalent").await;
}

struct PendingStartup(Arc<std::sync::atomic::AtomicBool>);
#[async_trait::async_trait]
impl HookRunner for PendingStartup {
    async fn run(&self, _: &HookInvocation) -> anyhow::Result<RawOutcome> {
        self.0.store(true, std::sync::atomic::Ordering::Release);
        std::future::pending().await
    }
}
#[tokio::test]
async fn native_session_cancelled_later_startup_handler_drains_earlier_idle_service() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let _lock = FIXTURES.lock().await;
    for reconcile in [false, true] {
        let mut host = host::Host::new(true);
        let token = "native-session-cancel-idle";
        let (package, service) = stdio_service(&host, token);
        let mut plans = plans(package, service.clone());
        let peer = peer();
        let end_package = super::package(HookDialect::Native);
        let end_service = managed(
            &host,
            end_package.clone(),
            ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
            1,
        );
        plans[1] = self::plans(end_package, end_service.clone()).remove(1);
        let entered = Arc::new(AtomicBool::new(false));
        let mut d = declaration("pause-startup", HookDialect::Native, HandlerClass::Observer);
        d.identity.runner = HandlerKind::Command;
        d.priority = 100;
        d.matcher = Matcher::default();
        plans[0].1.push(Registration {
            declaration: d,
            runner: Arc::new(PendingStartup(entered.clone())),
            revalidation: None,
        });
        let running = host.launch(plans);
        tokio::time::timeout(Duration::from_secs(3), async {
            while !entered.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("later startup handler did not start");
        let pids = processes(token);
        assert!(pids.len() >= 2);
        assert_eq!(service.state(), ServiceState::Ready);
        running
            .commands
            .send(demoncoder::session::Command::Cancel)
            .await
            .unwrap();
        host.ready().await;
        let cleaned_before_ready = pids
            .iter()
            .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists());
        let record = host.runtime.record().unwrap();
        let reconciled = if reconcile {
            host.runtime.reconcile("Fixture callback only waited; the confined peer and descendant were reaped, and no workspace changes occurred", None)
        } else {
            Ok(())
        };
        running.stop().await;
        // Even a deliberately failing control must join its owned work before assertions.
        service.stop().await.unwrap();
        end_service.stop().await.unwrap();
        assert!(
            cleaned_before_ready,
            "startup cancellation left earlier idle service processes alive"
        );
        assert!(record.operations.iter().any(|o|matches!(&o.host_invocation,Some(demoncoder::workflow::runtime::HostInvocation::NativeSession(lifetime)) if lifetime.end.is_none())),"startup cancel ended the host lifetime");
        assert!(record.operations.iter().any(|o| matches!(&o.host_invocation,
            Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(receipt))
                if !receipt.settled && receipt.hooks.iter().any(|h|
                    h.declaration.declaration == "pause-startup" && h.outcome.is_none() && h.uncertain_effects))),
            "interrupted command lost its unfinished unknown-effects receipt");
        reconciled.unwrap();
        host.assert_unspent();
        assert_eq!(
            peer.methods().iter().filter(|m| *m == "tools/call").count(),
            usize::from(reconcile),
            "independent end-only service must require non-held authority"
        );
        assert!(matches!(
            service.state(),
            ServiceState::Stopped | ServiceState::Failed
        ));
        assert_eq!(end_service.state(), ServiceState::Stopped);
    }
}
