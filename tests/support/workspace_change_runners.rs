use anyhow::{Context, Result, ensure};
use demoncoder::config::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

pub fn native_cwd_command_canary_registration() -> demoncoder::plugins::dispatch::Registration {
    use demoncoder::plugins::{
        self,
        dispatch::{Declaration, DeclarationIdentity, HandlerClass, Matcher, Scope},
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        runners::{CommandConfig, CommandProgram, CommandRunner},
    };
    use std::sync::Arc;

    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"native-cwd-command-canary","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        source.path().join("hook.py"),
        concat!(
            "import json,os,sys\n",
            "x=json.load(sys.stdin)\n",
            "assert x['hook_event_name']=='CwdChanged'\n",
            "assert x['demoncoder']['subject']['occurrence']['event']=='CwdChanged'\n",
            "assert os.getcwd()==x['new_cwd']\n",
            "print(json.dumps({'hookSpecificOutput':{'hookEventName':'CwdChanged','watchPaths':['/codex-ran-native-cwd-in-new-root']}}))\n",
        ),
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    let declaration = Declaration {
        required_gate: false,
        source: None,
        once: None,
        identity: DeclarationIdentity {
            package: "native-cwd-command-canary".into(),
            code: "captured-code".into(),
            policy: "captured-policy".into(),
            configuration: "captured-configuration".into(),
            generation: "1".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: "native-cwd-command-canary".into(),
            index: 0,
            dialect: HookDialect::Native,
            runner: HandlerKind::Command,
        },
        class: HandlerClass::Observer,
        priority: 0,
        matcher: Matcher::default(),
        reads: GateReadSet::default(),
        concurrent_group: None,
        read_only_endpoint: None,
        external_precondition: None,
    };
    let config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
    ]));
    CommandRunner::registration_for_event(package, declaration, HookEvent::CwdChanged, config, None)
        .unwrap()
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct ActualOwnerPeer {
    pub connection: Connection,
    root: PathBuf,
    _child: OwnedChild,
}

impl ActualOwnerPeer {
    pub async fn start(root: &Path, adapter: &str) -> Result<Self> {
        std::fs::create_dir_all(root.join("home"))?;
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut child = OwnedChild(
            Command::new("/usr/bin/python3")
                .arg(tests.join("plugin_batch_model.py"))
                .arg(root)
                .arg(adapter)
                .arg("workspace-owner")
                .stdout(Stdio::piped())
                .spawn()?,
        );
        let stdout = tokio::process::ChildStdout::from_std(
            child.0.stdout.take().context("owner peer stdout")?,
        )?;
        let mut ready = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::io::BufReader::new(stdout)
                .take(4097)
                .read_line(&mut ready),
        )
        .await
        .context("owner peer readiness timed out")??;
        ensure!(
            ready.ends_with('\n') && ready.len() <= 4096,
            "invalid owner peer readiness"
        );
        let ready: Value = serde_json::from_str(&ready)?;
        let endpoint = format!("http://127.0.0.1:{}", ready["port"]);
        let (variable, expected) = if adapter == "claude" {
            (
                "DEMONCODER_TEST_CLAUDE",
                "0399c793ff571d5946ef923d80b4f330d05ac4b6842a6b0775468f5d389403c0",
            )
        } else {
            (
                "DEMONCODER_TEST_CODEX",
                "c4d77a245a7fcda26f606bb4f726b59ede5fcf0eb322fb7d625a759fc150592a",
            )
        };
        let binary = PathBuf::from(
            std::env::var_os(variable).with_context(|| format!("{variable} is unset"))?,
        )
        .canonicalize()?;
        let mut source = std::fs::File::open(&binary)?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        ensure!(
            format!("{:x}", digest.finalize()) == expected,
            "{adapter} executable differs from pinned artifact"
        );
        std::fs::write(
            root.join("backend.json"),
            serde_json::to_vec(&json!({"binary":binary,"endpoint":endpoint,"ca":ready["ca"]}))?,
        )?;
        let launcher = tests.join(format!("plugin_{adapter}_launcher.py"));
        let relay = root.join(format!("plugin_{adapter}_artifact_relay.py"));
        std::fs::write(
            &relay,
            format!(
                "#!/usr/bin/python3\nimport os, sys\nos.environ['DEMONCODER_TEST_BACKEND_ROOT'] = {}\nos.execv('/usr/bin/python3', ['/usr/bin/python3', {}, *sys.argv[1:]])\n",
                serde_json::to_string(root.to_str().context("owner peer root is not UTF-8")?)?,
                serde_json::to_string(
                    launcher
                        .to_str()
                        .context("owner peer launcher path is not UTF-8")?
                )?
            ),
        )?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&relay, std::fs::Permissions::from_mode(0o700))?;
        let mut connection: Connection = serde_json::from_value(
            json!({"adapter":adapter,"model":if adapter == "claude" { "claude-sonnet-4-6" } else { "gpt-5.4" },"binary":relay}),
        )?;
        connection.access.supervisor = Some(PathBuf::from(env!("CARGO_BIN_EXE_demoncoder")));
        Ok(Self {
            connection,
            root: root.to_owned(),
            _child: child,
        })
    }

    pub fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.join("model-requests.jsonl"))
            .expect("installed owner request log")
            .lines()
            .map(|line| serde_json::from_str(line).expect("installed owner request JSON"))
            .collect()
    }

    pub fn assert_backend_workspace(&self, expected: &Path) {
        let pid = std::fs::read_to_string(self.root.join("backend.pid"))
            .expect("backend PID marker")
            .parse::<u32>()
            .expect("backend PID");
        let actual = std::fs::read_link(format!("/proc/{pid}/cwd"))
            .expect("live backend working directory")
            .canonicalize()
            .expect("canonical backend working directory");
        assert_eq!(
            actual,
            expected
                .canonicalize()
                .expect("canonical expected workspace"),
            "{} backend task left the admitted workspace",
            self.connection.adapter
        );
    }
}
