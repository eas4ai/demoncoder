use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use demoncoder::{
    config::{Args, Connection},
    events::EventSink,
    session::{
        ADAPTER_INTERFACE_VERSION, Command, Registry, Session, SessionCapabilities, TurnEnd,
    },
};
use tokio::sync::mpsc;

static FACTORY_CALLS: AtomicUsize = AtomicUsize::new(0);

struct Fixture;
#[async_trait]
impl Session for Fixture {
    fn owner(&self) -> &'static str {
        "fixture"
    }
    async fn turn(
        &mut self,
        _: String,
        _: &mut mpsc::Receiver<Command>,
        _: &EventSink,
    ) -> Result<TurnEnd> {
        panic!("capability admission must happen before model progression")
    }
}

fn factory(_: &Connection, _: &Path) -> Result<Box<dyn Session>> {
    FACTORY_CALLS.fetch_add(1, Ordering::SeqCst);
    Ok(Box::new(Fixture))
}

#[test]
fn declared_missing_controls_stop_before_factory_creation() {
    let workspace = tempfile::tempdir().unwrap();
    let config = workspace.path().join("settings.toml");
    std::fs::write(&config, "default_connection='selected'\n[connections.selected]\nadapter='limited'\nmodel='fixture'\n").unwrap();
    let args = Args::try_parse_from([
        "fixture",
        "--config",
        config.to_str().unwrap(),
        "--workspace",
        workspace.path().to_str().unwrap(),
        "--trust-workspace",
    ])
    .unwrap();
    let selected = args.selection().unwrap();
    assert_eq!(selected.name, "selected");
    for name in ["read", "write", "edit", "Bash", "steering", "cancellation"] {
        let mut capabilities = SessionCapabilities::CODING_SESSION;
        match name {
            "read" => capabilities.read = false,
            "write" => capabilities.write = false,
            "edit" => capabilities.edit = false,
            "Bash" => capabilities.bash = false,
            "steering" => capabilities.steering = false,
            _ => capabilities.cancellation = false,
        }
        let mut registry = Registry::default();
        registry
            .register_with_capabilities("limited", ADAPTER_INTERFACE_VERSION, capabilities, factory)
            .unwrap();
        let error = registry
            .open(&selected.connection, &selected.workspace)
            .err()
            .expect("missing capability was admitted");
        assert_eq!(
            error.to_string(),
            format!("selected adapter does not support {name}, which this coding session requires")
        );
        assert_eq!(FACTORY_CALLS.load(Ordering::SeqCst), 0);
    }
    let mut registry = Registry::default();
    assert!(
        registry
            .register("limited", ADAPTER_INTERFACE_VERSION + 1, factory)
            .is_err()
    );
    registry
        .register("limited", ADAPTER_INTERFACE_VERSION, factory)
        .unwrap();
    assert!(
        registry
            .register("limited", ADAPTER_INTERFACE_VERSION, factory)
            .is_err()
    );
    let session = registry
        .open(&selected.connection, &selected.workspace)
        .unwrap();
    assert_eq!(session.owner(), "fixture");
    assert_eq!(FACTORY_CALLS.load(Ordering::SeqCst), 1);
}
