use super::*;
#[path = "../support/session_transport.rs"]
mod host;

fn plans(peer: &Peer) -> Vec<(HookEvent, Vec<Registration>)> {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"session-http","version":"1.0.0"}"#,
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    [HookEvent::SessionStart, HookEvent::SessionEnd]
        .into_iter()
        .map(|event| {
            let mut d = declaration("session-http", HookDialect::Native, HandlerClass::Observer);
            d.matcher = Matcher::default();
            (
                event,
                vec![
                    HttpRunner::registration_for_event(
                        package.clone(),
                        d,
                        event,
                        HttpConfig::new(peer.endpoint.clone()),
                        None,
                    )
                    .unwrap(),
                ],
            )
        })
        .collect()
}
#[tokio::test]
async fn native_session_http_no_prompt_start_and_end_use_original_grant() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::response(200, b"{}".to_vec());
    let mut host = host::Host::new(true);
    host.start(plans(&peer)).await.stop().await;
    assert_eq!(peer.count(), 2, "native HTTP startup/end did not execute");
    let requests = peer.requests.lock().unwrap();
    assert_eq!(requests[0].1["hook_event_name"], "SessionStart");
    assert_eq!(requests[0].1["source"], "startup");
    assert_eq!(requests[1].1["hook_event_name"], "SessionEnd");
    assert_eq!(requests[1].1["reason"], "shutdown");
    assert!(host.runtime.record().unwrap().allocation.is_none());
    host.assert_unspent();
}
#[tokio::test]
async fn native_session_http_missing_grant_never_borrows_task_allowance() {
    let _lock = FIXTURES.lock().await;
    let peer = Peer::response(200, b"{}".to_vec());
    let mut host = host::Host::new(false);
    host.runtime
        .allocate(demoncoder::workflow::allocation::Limits::default(), None)
        .unwrap();
    host.start(plans(&peer)).await.stop().await;
    assert_eq!(peer.count(), 0);
    host.assert_unspent();
}
