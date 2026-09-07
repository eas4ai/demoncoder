use demoncoder::{
    events::{Event, EventSink},
    tools::{ToolCall, ToolExecutor},
};
use serde_json::json;
use tokio::sync::mpsc;

async fn output_case(steps: &[(u8, Vec<u8>)], expected_out: &str, expected_err: &str) {
    let root = tempfile::tempdir().unwrap();
    let mut script = String::from("import os,time\n");
    for (fd, bytes) in steps {
        script.push_str(&format!(
            "os.write({fd},bytes({bytes:?}))\ntime.sleep(0.075)\n"
        ));
    }
    std::fs::write(root.path().join("fixture.py"), script).unwrap();
    let log = root.path().join("events.jsonl");
    let executor = ToolExecutor::new(root.path()).unwrap();
    let (tx, mut rx) = mpsc::channel(256);
    let events = EventSink::new("utf8".into(), tx, Some(&log)).unwrap();
    let result = executor
        .execute(
            ToolCall {
                id: "utf8-call".into(),
                name: "bash".into(),
                arguments: json!({"command":"python3 fixture.py"}),
            },
            &events,
        )
        .await
        .unwrap();
    assert!(result.success, "{}", result.output);
    let (mut out, mut err, mut displayed) = (String::new(), String::new(), String::new());
    while let Ok(envelope) = rx.try_recv() {
        if let Event::ToolOutput { stream, text, .. } = envelope.event {
            match stream {
                "stdout" => out.push_str(&text),
                "stderr" => err.push_str(&text),
                other => panic!("{other}"),
            }
            displayed.push_str(&text);
        }
    }
    assert_eq!(out, expected_out, "stdout decoding");
    assert_eq!(err, expected_err, "stderr decoding");
    assert_eq!(
        result.output, displayed,
        "receipt must match displayed decoded fragments"
    );
    let recorded: Vec<serde_json::Value> = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let retained: String = recorded
        .iter()
        .filter(|v| v["event"]["type"] == "tool_output")
        .map(|v| v["event"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(retained, displayed);
    assert!(
        recorded
            .iter()
            .any(|v| v["event"]["type"] == "tool_finished"
                && v["event"]["result"]["output"] == result.output)
    );
}

#[tokio::test]
async fn bash_preserves_every_multibyte_split_boundary() {
    for character in ["é", "€", "🙂"] {
        let bytes = character.as_bytes();
        for split in 1..bytes.len() {
            for fd in [1, 2] {
                output_case(
                    &[(fd, bytes[..split].to_vec()), (fd, bytes[split..].to_vec())],
                    if fd == 1 { character } else { "" },
                    if fd == 2 { character } else { "" },
                )
                .await;
            }
        }
    }
}

#[tokio::test]
async fn bash_keeps_stream_decoders_separate_and_flushes_incomplete_eof() {
    output_case(&[(1, vec![0xe2]), (2, vec![0x82, 0xac])], "�", "��").await;
    output_case(
        &[
            (1, vec![0xe2]),
            (2, vec![0xf0, 0x9f]),
            (1, vec![0x82, 0xac]),
            (2, vec![0x99, 0x82]),
        ],
        "€",
        "🙂",
    )
    .await;
    output_case(
        &[
            (1, vec![0xff, b'a', 0xe2]),
            (1, vec![b'b', 0xf0, 0x9f]),
            (2, vec![0xc3]),
        ],
        "�a�b�",
        "�",
    )
    .await;
}
