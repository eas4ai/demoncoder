use std::{collections::BTreeSet, os::unix::fs::PermissionsExt, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::{
    adapters::{
        http,
        process::{BackendProcess, executable},
    },
    config::Connection,
};

const BYTE_LIMIT: usize = 1024 * 1024;
const MODEL_LIMIT: usize = 2048;
const PAGE_LIMIT: usize = 16;

pub(crate) struct Catalog {
    pub models: Vec<String>,
    pub note: String,
}

pub(crate) async fn check(connection: &Connection) -> Result<Catalog> {
    check_with_timeout(connection, Duration::from_secs(15)).await
}

async fn check_with_timeout(connection: &Connection, deadline: Duration) -> Result<Catalog> {
    connection.validate()?;
    tokio::time::timeout(deadline, async {
        match connection.adapter.as_str() {
            "openai-api" | "anthropic-api" => api(connection).await,
            "codex" | "claude" => backend(connection).await,
            _ => bail!("unsupported provider connection"),
        }
    })
    .await
    .context("provider check timed out; retry after checking connectivity")?
}

fn identifier(value: &Value) -> Result<String> {
    let value = value
        .as_str()
        .context("provider returned an invalid model identifier")?;
    ensure!(
        !value.is_empty() && value.len() <= 256 && value.chars().all(|c| c.is_ascii_graphic()),
        "provider returned an invalid model identifier"
    );
    Ok(value.to_owned())
}

fn append_models(result: &Value, field: &str, models: &mut BTreeSet<String>) -> Result<()> {
    let entries = result["data"]
        .as_array()
        .context("provider did not return a model list")?;
    ensure!(
        entries.len() <= MODEL_LIMIT,
        "provider returned too many models"
    );
    for entry in entries {
        models.insert(identifier(&entry[field])?);
    }
    ensure!(
        models.len() <= MODEL_LIMIT,
        "provider returned too many models"
    );
    Ok(())
}

fn catalog(models: BTreeSet<String>, note: &str) -> Result<Catalog> {
    ensure!(
        !models.is_empty(),
        "provider returned no models; check account access"
    );
    Ok(Catalog {
        models: models.into_iter().collect(),
        note: note.into(),
    })
}

async fn api(connection: &Connection) -> Result<Catalog> {
    let anthropic = connection.adapter == "anthropic-api";
    let key = connection.api_key(if anthropic {
        "ANTHROPIC_API_KEY"
    } else {
        "OPENAI_API_KEY"
    })?;
    let endpoint = connection.endpoint.as_deref().unwrap_or(if anthropic {
        "https://api.anthropic.com/v1/messages"
    } else {
        "https://api.openai.com/v1/responses"
    });
    let endpoint = http::endpoint(endpoint)?;
    let mut url = endpoint
        .join("models")
        .context("cannot derive models endpoint")?;
    let client = http::client()?;
    let mut models = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut remaining = BYTE_LIMIT;
    for _ in 0..PAGE_LIMIT {
        let mut request = client.get(url.clone());
        request = if anthropic {
            request
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01")
        } else {
            request.bearer_auth(&key)
        };
        let response = http::response(request).await?;
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow::anyhow!("model list transfer failed"))?;
            ensure!(chunk.len() <= remaining, "model catalog exceeds 1 MiB");
            remaining -= chunk.len();
            bytes.extend_from_slice(&chunk);
        }
        let result: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("provider returned invalid model JSON"))?;
        append_models(&result, "id", &mut models)?;
        ensure!(
            !models.iter().any(|model| model.contains(&key)),
            "provider model catalog contained credential material"
        );
        match result.get("has_more") {
            None | Some(Value::Bool(false)) => {
                return catalog(
                    models,
                    "Authentication accepted. Advertised models are listed; individual model access is not guaranteed.",
                );
            }
            Some(Value::Bool(true)) => {}
            _ => bail!("provider returned invalid model pagination"),
        }
        let cursor = identifier(&result["last_id"])?;
        ensure!(
            cursors.insert(cursor.clone()),
            "provider repeated a model page"
        );
        url.set_query(None);
        url.query_pairs_mut()
            .append_pair(if anthropic { "after_id" } else { "after" }, &cursor);
    }
    bail!("model catalog exceeds pagination limit")
}

async fn rpc(
    process: &mut BackendProcess,
    id: u64,
    method: &str,
    params: Value,
    remaining: &mut usize,
) -> Result<Value> {
    process
        .send(json!({"id":id,"method":method,"params":params}))
        .await?;
    for _ in 0..64 {
        let message = process.receive_limited(*remaining).await?;
        let size = process.last_response_bytes();
        ensure!(size <= *remaining, "backend catalog exceeds 1 MiB");
        *remaining -= size;
        if message.get("method").is_some() && message.get("id").is_some() {
            process.send(json!({"id":message["id"],"error":{"code":-32601,"message":"Settings check does not execute requests"}})).await?;
        } else if message["id"] == id {
            ensure!(
                message.get("error").is_none(),
                "backend rejected the authentication or model check"
            );
            return message
                .get("result")
                .cloned()
                .context("backend check returned no result");
        }
    }
    bail!("backend check exceeded message limit")
}

async fn backend(connection: &Connection) -> Result<Catalog> {
    ensure!(
        connection.endpoint.is_none(),
        "subscription connections do not use API endpoints"
    );
    let binary = executable(connection.binary.as_deref(), &connection.adapter)?;
    // TMPDIR can point inside the coding workspace, where parent configuration
    // would still be discovered. Use the platform's neutral Unix temp root.
    let cwd = tempfile::Builder::new()
        .prefix("demoncoder-provider-check-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .context("create neutral provider-check directory")?;
    let codex = connection.adapter == "codex";
    let args: Vec<String> = if codex {
        [
            "app-server",
            "--stdio",
            "-c",
            "forced_login_method=\"chatgpt\"",
            "--disable",
            "hooks",
            "--disable",
            "plugins",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    } else {
        ["auth", "status"].into_iter().map(str::to_owned).collect()
    };
    // Only Codex's managed login location is forwarded. API billing, custom
    // Claude config locations, OAuth token overrides and cloud routes are absent.
    let mut process = BackendProcess::spawn(
        &binary,
        &args,
        cwd.path(),
        if codex { &["CODEX_HOME"] } else { &[] },
    )?;
    let result = if codex {
        codex_catalog(&mut process).await
    } else {
        claude_catalog(&mut process).await
    };
    process.stop().await?;
    result
}

async fn claude_catalog(process: &mut BackendProcess) -> Result<Catalog> {
    let status = process.finite_json(BYTE_LIMIT).await?;
    ensure!(
        status["loggedIn"] == true
            && status["authMethod"] == "claude.ai"
            && status.get("apiKeySource").is_none_or(Value::is_null),
        "Claude requires its managed subscription login; run claude auth login"
    );
    // Official CLI reference documents auth status; model-config documents these
    // aliases. They are deliberately not represented as account model discovery.
    catalog(
        ["haiku", "opus", "sonnet"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        "Claude reports a managed subscription login. These are supported backend aliases, not account-discovered models; access is not guaranteed.",
    )
}

async fn codex_catalog(process: &mut BackendProcess) -> Result<Catalog> {
    let mut remaining = BYTE_LIMIT;
    rpc(
        process,
        1,
        "initialize",
        json!({"clientInfo":{"name":"demoncoder-settings","version":env!("CARGO_PKG_VERSION")}}),
        &mut remaining,
    )
    .await?;
    process
        .send(json!({"method":"initialized","params":{}}))
        .await?;
    let configuration = rpc(
        process,
        2,
        "config/read",
        json!({"includeLayers":false}),
        &mut remaining,
    )
    .await?;
    validate_codex_route(&configuration["config"])?;
    let account = rpc(
        process,
        3,
        "account/read",
        json!({"refreshToken":true}),
        &mut remaining,
    )
    .await?;
    ensure!(
        account["account"]["type"] == "chatgpt" && account["requiresOpenaiAuth"] == true,
        "Codex requires a ChatGPT subscription login; run codex login"
    );
    let mut models = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut cursor = Value::Null;
    for page in 0..PAGE_LIMIT {
        let result = rpc(
            process,
            page as u64 + 4,
            "model/list",
            json!({"cursor":cursor,"limit":100}),
            &mut remaining,
        )
        .await?;
        append_models(&result, "model", &mut models)?;
        match result.get("nextCursor") {
            None | Some(Value::Null) => {
                return catalog(
                    models,
                    "ChatGPT login checked. Backend-advertised models are listed; individual model access is not guaranteed.",
                );
            }
            Some(value) => {
                let next = identifier(value)?;
                ensure!(
                    cursors.insert(next.clone()),
                    "backend repeated a model page"
                );
                cursor = next.into();
            }
        }
    }
    bail!("backend catalog exceeds pagination limit")
}

/// Both Settings discovery and coding connections must use the same managed
/// subscription route. Login type alone does not constrain inherited routing.
pub(crate) fn validate_codex_route(configuration: &Value) -> Result<()> {
    const CHATGPT_BASE_URL: &str = "https://chatgpt.com/backend-api/";

    let config = configuration
        .as_object()
        .context("Codex did not return its effective configuration")?;
    ensure!(
        config
            .get("model_provider")
            .is_none_or(|value| value.is_null() || value == "openai"),
        "Codex subscription requires the built-in OpenAI provider; remove the custom model_provider from Codex configuration"
    );
    if let Some(providers) = config
        .get("model_providers")
        .filter(|value| !value.is_null())
    {
        let providers = providers
            .as_object()
            .context("Codex returned invalid provider configuration")?;
        ensure!(
            !providers.contains_key("openai"),
            "Codex subscription does not accept a custom OpenAI provider definition; remove it from Codex configuration"
        );
    }
    ensure!(
        config.get("openai_base_url").is_none_or(Value::is_null),
        "Codex subscription does not accept a custom service URL; remove the base URL override from Codex configuration"
    );
    ensure!(
        config
            .get("chatgpt_base_url")
            .is_none_or(|value| value.is_null() || value == CHATGPT_BASE_URL),
        "Codex subscription does not accept a custom service URL; remove the base URL override from Codex configuration"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        os::unix::fs::PermissionsExt,
        path::Path,
    };

    fn connection(adapter: &str) -> Connection {
        serde_json::from_value(json!({"adapter":adapter,"api_key": if adapter.ends_with("-api") { Some("fixture-key") } else { None }})).unwrap()
    }

    fn script(dir: &Path, code: &str) -> Result<std::path::PathBuf> {
        let path = dir.join("backend");
        std::fs::write(
            &path,
            format!(
                "#!/usr/bin/python3\nimport sys\nif sys.argv[1:] == ['--fixture-ready']: sys.exit(0)\n{code}\n"
            ),
        )?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        // A concurrent fork can briefly inherit the writer before close-on-exec.
        // Wait until this fixture is executable before exercising the real probe.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match std::process::Command::new(&path)
                .arg("--fixture-ready")
                .status()
            {
                Ok(status) => {
                    anyhow::ensure!(status.success(), "backend fixture readiness failed");
                    break;
                }
                Err(error)
                    if error.raw_os_error() == Some(rustix::io::Errno::TXTBSY.raw_os_error())
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(path)
    }

    #[tokio::test]
    async fn native_catalog_checks_http_auth_and_rejects_bad_responses() -> Result<()> {
        for (status, body, good) in [
            (200, r#"{"data":[{"id":"fixture-model"}]}"#, true),
            (401, "fixture-key reflected", false),
            (302, "redirect forbidden", false),
            (200, "fixture-key is not JSON", false),
            (200, r#"{"data":[{"id":"bad\u001b[2J"}]}"#, false),
            (200, r#"{"data":[]}"#, false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut bytes = [0; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let count = socket.read(&mut bytes).unwrap();
                    assert_ne!(count, 0);
                    request.extend_from_slice(&bytes[..count]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("GET /v1/models HTTP/1.1"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains("authorization: bearer ")
                );
                write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let mut config = connection("openai-api");
            config.endpoint = Some(format!("http://{address}/v1/responses"));
            let result = check(&config).await;
            server.join().unwrap();
            assert_eq!(result.is_ok(), good);
            if let Err(error) = result {
                assert!(!format!("{error:#}").contains("fixture-key"));
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn backend_checks_never_submit_coding_work() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut config = connection("codex");
        config.binary = Some(script(
            dir.path(),
            r#"
import sys,json,os
assert sys.argv[1:3] == ['app-server','--stdio']
assert not os.path.exists('.git')
assert os.stat('.').st_mode & 0o777 == 0o700
assert 'OPENAI_API_KEY' not in os.environ
assert 'ANTHROPIC_API_KEY' not in os.environ
for line in sys.stdin:
 r=json.loads(line); method=r.get('method')
 if method=='initialized': continue
 assert method in ['initialize','config/read','account/read','model/list']
 if method=='initialize': result={}
 elif method=='config/read': result={'config':{}}
 elif method=='account/read':
  assert r['params']['refreshToken'] is True
  result={'account':{'type':'chatgpt'}, 'requiresOpenaiAuth':True}
 else:
  cursor=r['params']['cursor']
  result={'data':[{'model':'second' if cursor else 'first'}], 'nextCursor':None if cursor else 'page2'}
 print(json.dumps({'id':r['id'],'result':result}),flush=True)
"#,
        )?);
        assert_eq!(check(&config).await?.models, ["first", "second"]);
        let mut config = connection("claude");
        config.binary = Some(script(
            dir.path(),
            r#"
import sys,json,os
assert sys.argv[1:] == ['auth','status']
assert 'CLAUDE_CONFIG_DIR' not in os.environ
assert 'CLAUDE_CODE_OAUTH_TOKEN' not in os.environ
assert 'ANTHROPIC_API_KEY' not in os.environ
assert not os.path.exists('.git')
assert os.stat('.').st_mode & 0o777 == 0o700
print(json.dumps({'loggedIn':True,'authMethod':'claude.ai'},indent=2))
"#,
        )?);
        let result = check(&config).await?;
        assert_eq!(result.models, ["haiku", "opus", "sonnet"]);
        assert!(result.note.contains("not account-discovered"));
        Ok(())
    }

    #[tokio::test]
    async fn anthropic_catalog_follows_only_bounded_same_origin_pages() -> Result<()> {
        for repeat in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let server = std::thread::spawn(move || {
                for page in 0..2 {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut bytes = [0; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let count = socket.read(&mut bytes).unwrap();
                        assert_ne!(count, 0);
                        request.extend_from_slice(&bytes[..count]);
                    }
                    let request = String::from_utf8(request).unwrap();
                    assert!(request.contains("x-api-key:"));
                    assert!(request.contains("anthropic-version: 2023-06-01"));
                    assert!(!request.contains("authorization:"));
                    if page == 1 {
                        assert!(request.starts_with("GET /v1/models?after_id=first "));
                    }
                    let body = json!({"data":[{"id":if page == 0 { "first" } else { "second" }}], "has_more":page == 0 || repeat, "last_id":"first"}).to_string();
                    write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                }
            });
            let mut config = connection("anthropic-api");
            config.endpoint = Some(format!("http://{address}/v1/messages"));
            let result = check(&config).await;
            server.join().unwrap();
            if repeat {
                assert!(result.err().unwrap().to_string().contains("repeated"));
            } else {
                assert_eq!(result?.models, ["first", "second"]);
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn native_catalog_bounds_transfer_bytes_and_total_wait() -> Result<()> {
        for slow in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut buffer = [0; 8192];
                assert!(socket.read(&mut buffer).unwrap() > 0);
                if slow {
                    std::thread::sleep(Duration::from_millis(200));
                }
                let body = if slow {
                    r#"{"data":[{"id":"model"}]}"#.to_owned()
                } else {
                    " ".repeat(BYTE_LIMIT + 1)
                };
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
            let mut config = connection("openai-api");
            config.endpoint = Some(format!("http://{address}/v1/responses"));
            let error =
                check_with_timeout(&config, Duration::from_millis(if slow { 50 } else { 3000 }))
                    .await
                    .err()
                    .context("unbounded native check accepted")?;
            server.join().unwrap();
            assert!(
                error
                    .to_string()
                    .contains(if slow { "timed out" } else { "1 MiB" })
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn native_catalog_does_not_publish_reflected_credentials() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut bytes = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = socket.read(&mut bytes).unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&bytes[..count]);
            }
            let request = String::from_utf8(request).unwrap();
            let key = request
                .lines()
                .find_map(|line| line.strip_prefix("authorization: Bearer "))
                .unwrap();
            let body = json!({"data":[{"id":format!("reflected-{key}-model")}]}).to_string();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let mut config = connection("openai-api");
        config.endpoint = Some(format!("http://{address}/v1/responses"));
        let error = check(&config)
            .await
            .err()
            .context("credential reflection was accepted")?;
        server.join().unwrap();
        assert_eq!(
            error.to_string(),
            "provider model catalog contained credential material"
        );
        Ok(())
    }

    #[tokio::test]
    async fn backend_rejects_missing_billing_malformed_slow_and_oversize_status() -> Result<()> {
        let dir = tempfile::tempdir()?;
        for body in [
            "print('{\"loggedIn\":false,\"authMethod\":\"none\"}')",
            "print('{\"loggedIn\":true,\"authMethod\":\"api_key\"}')",
            "print('{\"loggedIn\":true,\"authMethod\":\"claude.ai\",\"apiKeySource\":\"managed key\"}')",
            "print('reflected-fixture-secret')",
            "import time; time.sleep(5)",
            "print('x' * (1024 * 1024 + 1))",
            "print('{\"loggedIn\":true,\"authMethod\":\"claude.ai\"}'); raise SystemExit(1)",
        ] {
            let mut config = connection("claude");
            config.binary = Some(script(dir.path(), body)?);
            let error = check_with_timeout(&config, Duration::from_millis(150))
                .await
                .err()
                .context("bad backend was accepted")?;
            assert!(!format!("{error:#}").contains("reflected-fixture-secret"));
        }
        Ok(())
    }

    #[tokio::test]
    async fn codex_declines_requests_and_rejects_non_subscription_login() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut config = connection("codex");
        config.binary = Some(script(
            dir.path(),
            r#"
import sys,json
r=json.loads(input())
print(json.dumps({'id':55,'method':'item/tool/call','params':{}}),flush=True)
decline=json.loads(input()); assert decline['id']==55 and 'error' in decline
print(json.dumps({'id':r['id'],'result':{}}),flush=True)
assert json.loads(input())['method']=='initialized'
r=json.loads(input()); assert r['method']=='config/read'
print(json.dumps({'id':r['id'],'result':{'config':{}}}),flush=True)
r=json.loads(input()); assert r['method']=='account/read'
print(json.dumps({'id':r['id'],'result':{'account':{'type':'apiKey'}}}),flush=True)
"#,
        )?);
        assert!(
            check(&config)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("ChatGPT")
        );
        Ok(())
    }

    #[test]
    fn codex_rejects_conflicting_subscription_routes() {
        for config in [
            json!({}),
            json!({"model_provider":"openai","model_providers":{}}),
            json!({"chatgpt_base_url":"https://chatgpt.com/backend-api/"}),
        ] {
            assert!(validate_codex_route(&config).is_ok());
        }
        for config in [
            Value::Null,
            json!({"model_provider":"local"}),
            json!({"model_providers":{"openai":{}}}),
            json!({"model_providers":[]}),
            json!({"chatgpt_base_url":"https://other.invalid"}),
            json!({"chatgpt_base_url":"https://chatgpt.com/backend-api"}),
            json!({"chatgpt_base_url":"https://chatgpt.com/backend-api/fixture"}),
            json!({"openai_base_url":"https://other.invalid"}),
        ] {
            assert!(validate_codex_route(&config).is_err());
        }
    }

    #[tokio::test]
    async fn codex_does_not_accept_a_login_on_another_provider_route() -> Result<()> {
        let dir = tempfile::tempdir()?;
        for wrong_config in [false, true] {
            let mut config = connection("codex");
            let code = format!(
                r#"
import sys,json
wrong_config={}
for line in sys.stdin:
 r=json.loads(line); method=r.get('method')
 if method=='initialized': continue
 if method=='initialize': result={{}}
 elif method=='config/read': result={{'config':{{'model_provider':'other' if wrong_config else 'openai'}}}}
 elif method=='account/read': result={{'account':{{'type':'chatgpt'}},'requiresOpenaiAuth':wrong_config}}
 elif method=='model/list': result={{'data':[{{'model':'must-not-be-offered'}}]}}
 else: raise RuntimeError('unexpected method')
 print(json.dumps({{'id':r['id'],'result':result}}),flush=True)
"#,
                if wrong_config { "True" } else { "False" }
            );
            config.binary = Some(script(dir.path(), &code)?);
            assert!(check(&config).await.is_err());
        }
        Ok(())
    }
}
