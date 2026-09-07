//! Launched with ambient descriptors by tests/inherited_fds.py.
use demoncoder::{
    events::EventSink,
    tools::{ToolCall, ToolExecutor},
};
use serde_json::json;
use std::path::Path;
use tokio::sync::mpsc;

#[tokio::test]
#[ignore = "requires inherited descriptor fixtures from tests/inherited_fds.py"]
async fn confined_executor_closes_ambient_descriptors() {
    let workspace = std::env::var("PROBE_WORKSPACE").unwrap();
    let socket: u32 = std::env::var("PROBE_SOCKET_FD").unwrap().parse().unwrap();
    let file: u32 = std::env::var("PROBE_FILE_FD").unwrap().parse().unwrap();
    let executor = ToolExecutor::new(Path::new(&workspace)).unwrap();
    let (tx, _rx) = mpsc::channel(16);
    let events = EventSink::new("inherited-fd".into(), tx, None).unwrap();
    let script = format!(
        r#"python3 - <<'PYTHON'
import errno, os
for fd in [{socket}, {file}]:
    try:
        os.write(fd, b'HOST-DESCRIPTOR-CONTACTED')
    except OSError as error:
        assert error.errno == errno.EBADF, error
    else:
        raise AssertionError('ambient descriptor survived: ' + str(fd))
print('AMBIENT-DESCRIPTORS-CLOSED')
PYTHON"#
    );
    let result = executor
        .execute(
            ToolCall {
                id: "ambient-fd".into(),
                name: "bash".into(),
                arguments: json!({"command":script}),
            },
            &events,
        )
        .await
        .unwrap();
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("AMBIENT-DESCRIPTORS-CLOSED"));
}
