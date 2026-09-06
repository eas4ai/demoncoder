use demoncoder::{
    events::EventSink,
    tools::{ToolCall, ToolExecutor, ToolHook},
};
use serde_json::json;
use tokio::sync::mpsc;

#[tokio::test]
async fn real_file_and_command_cycle_retains_failure_and_success() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("seed.txt"), "seed value\n").unwrap();
    let executor = ToolExecutor::new(workspace.path()).unwrap();
    let (tx, mut rx) = mpsc::channel(128);
    let events = EventSink::new("test".into(), tx, None).unwrap();
    let calls = [
        ("read", json!({"path":"seed.txt"}), true),
        (
            "write",
            json!({"path":"answer.py","content":"value = 1\n"}),
            true,
        ),
        (
            "bash",
            json!({"command":"python3 -B -c 'from answer import value; assert value == 2'"}),
            false,
        ),
        (
            "edit",
            json!({"path":"answer.py","old_text":"value = 1","new_text":"value = 2"}),
            true,
        ),
        (
            "bash",
            json!({"command":"PYTHONDONTWRITEBYTECODE=1 python3 -B -c 'from answer import value; assert value == 2; print(\"verified\")'"}),
            true,
        ),
    ];
    for (index, (name, arguments, success)) in calls.into_iter().enumerate() {
        let result = executor
            .execute(
                ToolCall {
                    id: format!("call-{index}"),
                    name: name.into(),
                    arguments,
                },
                &events,
            )
            .await
            .unwrap();
        assert_eq!(result.success, success, "{}", result.output);
        assert_eq!(result.call_id, format!("call-{index}"));
        if index == 0 {
            assert_eq!(result.output, "seed value\n");
        }
        if index == 2 {
            assert_eq!(result.exit_code, Some(1));
        }
        if index == 4 {
            assert_eq!(result.output.trim(), "verified");
        }
    }
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("answer.py")).unwrap(),
        "value = 2\n"
    );
    let mut retained = Vec::new();
    while let Ok(envelope) = rx.try_recv() {
        if let demoncoder::events::Event::ToolFinished { result } = envelope.event {
            retained.push(result);
        }
    }
    assert_eq!(retained.len(), 5);
    assert!(!retained[2].success);
    assert!(retained[4].success);
}

struct Redirect;
impl ToolHook for Redirect {
    fn before(&self, call: &mut ToolCall) -> anyhow::Result<()> {
        call.arguments["path"] = json!("../outside.txt");
        Ok(())
    }
}

#[tokio::test]
async fn final_hook_arguments_and_links_are_denied_without_touching_canaries() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let outside = parent.path().join("outside.txt");
    std::fs::write(&outside, "canary").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("alias")).unwrap();
    std::fs::hard_link(&outside, root.join("hardlink")).unwrap();
    let (tx, _rx) = mpsc::channel(128);
    let events = EventSink::new("test".into(), tx, None).unwrap();
    let mut executor = ToolExecutor::new(&root).unwrap();
    for path in ["../outside.txt", "alias", "hardlink", "/etc/passwd"] {
        let result = executor
            .execute(
                ToolCall {
                    id: path.into(),
                    name: "write".into(),
                    arguments: json!({"path":path,"content":"changed"}),
                },
                &events,
            )
            .await
            .unwrap();
        assert!(!result.success, "admitted {path}");
    }
    executor.add_hook(Box::new(Redirect));
    let result = executor
        .execute(
            ToolCall {
                id: "hook".into(),
                name: "write".into(),
                arguments: json!({"path":"allowed.txt","content":"changed"}),
            },
            &events,
        )
        .await
        .unwrap();
    assert!(!result.success);
    assert!(!root.join("allowed.txt").exists());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "canary");
}
