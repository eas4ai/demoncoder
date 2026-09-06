//! Interactive setup writes one private settings transaction before session startup.
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, IsTerminal, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::config::{Args, Config, Connection, OracleAssignment};

pub fn prepare(args: &Args) -> Result<()> {
    let config = args.load_config()?;
    let workspace = args.workspace.canonicalize().context("resolve workspace")?;
    ensure!(workspace.is_dir(), "workspace must be a directory");
    let setup = args.setup || (args.config.is_none() && !config.onboarding_complete);
    let trusted = args.yolo
        || args.trust_workspace
        || config
            .trusted_workspaces
            .iter()
            .any(|root| workspace.starts_with(root));
    if !setup && trusted {
        return Ok(());
    }
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "first setup and new project trust require an interactive terminal; automated invocations need --config and --trust-workspace"
    );
    let path = args
        .config_path()
        .context("set HOME or select --config for private settings")?;
    let _lock = settings_lock(&path, args.config.is_none())?;
    // Another setup may have completed before we obtained its lock.
    let mut config = args.load_config()?;
    println!("DemonCoder setup");
    println!("Project: {:?}", workspace);
    println!(
        "Access: {}",
        if args.yolo {
            "HOST ACCESS -- no sandbox or routine tool prompts; Oracle guards outside access"
        } else {
            "confined project -- read, write, edit, and Bash"
        }
    );
    if !args.yolo
        && !args.trust_workspace
        && !config
            .trusted_workspaces
            .iter()
            .any(|root| workspace.starts_with(root))
    {
        ensure!(
            yes("Trust this project for coding tools?", false)?,
            "project trust declined; no session started"
        );
        config.trusted_workspaces.push(workspace);
    }
    if setup {
        configure(args, &mut config)?;
    }
    save(&path, &config)?;
    println!("Private settings saved. Starting the selected session.\n");
    Ok(())
}

fn configure(args: &Args, config: &mut Config) -> Result<()> {
    loop {
        println!(
            "Providers: 1 Codex subscription · 2 Claude subscription · 3 OpenAI API · 4 Anthropic API"
        );
        let selected = args
            .connection
            .as_ref()
            .or(config.default_connection.as_ref());
        let current = selected.and_then(|name| config.connections.get(name));
        let default = match current.map(|c| c.adapter.as_str()) {
            Some("claude") => "2",
            Some("openai-api") => "3",
            Some("anthropic-api") => "4",
            _ => "1",
        };
        let adapter = loop {
            let choice = line("Provider", default)?;
            match choice.as_str() {
                "1" => break "codex",
                "2" => break "claude",
                "3" => break "openai-api",
                "4" => break "anthropic-api",
                _ => println!("Choose 1, 2, 3, or 4."),
            }
        };
        let name = loop {
            let name = line("Connection name", adapter)?;
            if name.len() <= 64
                && !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                break name;
            }
            println!("Use up to 64 letters, digits, hyphens, or underscores.");
        };
        let mut connection = config
            .connections
            .get(&name)
            .filter(|c| c.adapter == adapter)
            .cloned()
            .unwrap_or(Connection {
                adapter: adapter.into(),
                model: None,
                endpoint: None,
                binary: None,
                effort: None,
                api_key: None,
                access: Default::default(),
            });
        if adapter.ends_with("-api") {
            let variable = if adapter == "openai-api" {
                "OPENAI_API_KEY"
            } else {
                "ANTHROPIC_API_KEY"
            };
            match std::env::var(variable) {
                Ok(value) if !value.trim().is_empty() => {
                    println!("Using {variable}; environment credentials take precedence.")
                }
                Ok(_) => bail!("{variable} is empty; set it or unset it before setup"),
                Err(std::env::VarError::NotUnicode(_)) => bail!("{variable} must be UTF-8"),
                Err(std::env::VarError::NotPresent) => loop {
                    let key = secret(if connection.api_key.is_some() {
                        "API key (Enter keeps the saved key)"
                    } else {
                        "API key"
                    })?;
                    if !key.is_empty() {
                        connection.api_key = Some(key);
                    }
                    if connection
                        .api_key
                        .as_ref()
                        .is_some_and(|key| !key.trim().is_empty())
                    {
                        break;
                    }
                    println!("An API key is required for this connection.");
                },
            }
        } else {
            println!(
                "Uses the existing {} login. If needed, run `{}` before starting work.",
                adapter,
                if adapter == "codex" {
                    "codex login"
                } else {
                    "claude auth login"
                }
            );
        }
        let default_model = match adapter {
            "openai-api" => "gpt-6-astra",
            "anthropic-api" => "claude-sonnet-4-6",
            _ => "backend-default",
        };
        let model = if let Some(model) = &args.model {
            model.clone()
        } else {
            line(
                "Model ID",
                connection.model.as_deref().unwrap_or(default_model),
            )?
        };
        connection.model = if model == "backend-default" && !adapter.ends_with("-api") {
            None
        } else {
            Some(model)
        };
        connection.effort = if args.effort.is_some() {
            args.effort.clone()
        } else {
            effort(&connection)?
        };
        connection.validate()?;
        config.connections.insert(name.clone(), connection);
        config.default_connection = Some(name);
        if !yes("Add another connection?", false)? {
            break;
        }
    }
    let names: Vec<_> = config.connections.keys().cloned().collect();
    println!("Configured connections: {}", names.join(", "));
    config.default_connection = Some(connection_name(
        "Default connection",
        config.default_connection.as_deref().unwrap_or(&names[0]),
        config,
    )?);
    let oracle_name = connection_name(
        "Oracle connection for outside access with --yolo",
        config
            .oracle
            .as_ref()
            .map(|o| o.connection.as_str())
            .unwrap_or(
                config
                    .default_connection
                    .as_deref()
                    .expect("default selected"),
            ),
        config,
    )?;
    let mut oracle = config.connections[&oracle_name].clone();
    if let Some(saved) = &config.oracle
        && saved.connection == oracle_name
    {
        if saved.model.is_some() {
            oracle.model = saved.model.clone();
        }
        if saved.effort.is_some() {
            oracle.effort = saved.effort.clone();
        }
    }

    let model = line(
        "Oracle model ID",
        oracle.model.as_deref().unwrap_or("backend-default"),
    )?;
    oracle.model = if model == "backend-default" && !oracle.adapter.ends_with("-api") {
        None
    } else {
        Some(model)
    };
    oracle.effort = effort(&oracle)?;
    oracle.validate()?;
    config.oracle = Some(OracleAssignment {
        connection: oracle_name,
        model: oracle.model,
        effort: oracle.effort,
    });
    config.onboarding_complete = true;
    println!("Extensions: no external plugins or hooks are enabled or loaded automatically.");
    Ok(())
}

fn connection_name(label: &str, default: &str, config: &Config) -> Result<String> {
    loop {
        let name = line(label, default)?;
        if config.connections.contains_key(&name) {
            return Ok(name);
        }
        println!("Choose one of the configured connection names.");
    }
}

fn effort(connection: &Connection) -> Result<Option<String>> {
    println!(
        "Effort: default, low, medium, high, xhigh, max{}",
        if matches!(connection.adapter.as_str(), "codex" | "openai-api") {
            ", none, minimal, ultra, persistent"
        } else {
            ""
        }
    );
    loop {
        let value = line(
            "Thinking/response effort",
            connection.effort.as_deref().unwrap_or("default"),
        )?;
        let value = (value != "default").then_some(value);
        let mut candidate = connection.clone();
        candidate.effort = value.clone();
        if candidate.validate().is_ok() {
            return Ok(value);
        }
        println!("That effort is unsupported by the selected adapter.");
    }
}

fn yes(label: &str, default: bool) -> Result<bool> {
    loop {
        match line(label, if default { "y" } else { "N" })?
            .to_ascii_lowercase()
            .as_str()
        {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Enter y or n."),
        }
    }
}

fn line(label: &str, default: &str) -> Result<String> {
    let safe = |text: &str| {
        text.chars()
            .map(|c| if c.is_control() { '\u{fffd}' } else { c })
            .collect::<String>()
    };
    print!("{} [{}]: ", safe(label), safe(default));
    std::io::stdout().flush()?;
    let mut value = String::new();
    let count = std::io::stdin().lock().take(4097).read_line(&mut value)?;
    ensure!(count > 0, "setup cancelled before saving");
    ensure!(
        count <= 4096 && value.ends_with('\n'),
        "setup input exceeds 4096 bytes"
    );
    let value = value.trim();
    Ok(if value.is_empty() {
        default.into()
    } else {
        value.into()
    })
}

struct SecretInput;
impl Drop for SecretInput {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn secret(label: &str) -> Result<String> {
    crossterm::terminal::enable_raw_mode()?;
    let _restore = SecretInput;
    print!("{label}: ");
    std::io::stdout().flush()?;
    let mut value = String::new();
    loop {
        if let Event::Key(key) = crossterm::event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Enter => {
                    print!("\r\n");
                    std::io::stdout().flush()?;
                    return Ok(value.trim().into());
                }
                KeyCode::Esc => bail!("setup cancelled before saving"),
                KeyCode::Char('c' | 'd') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    bail!("setup cancelled before saving")
                }
                KeyCode::Backspace if !value.is_empty() => {
                    value.pop();
                    print!("\x08 \x08");
                }
                KeyCode::Char(c)
                    if !c.is_control()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    ensure!(
                        value.len() + c.len_utf8() <= 4096,
                        "API key exceeds 4096 bytes"
                    );
                    value.push(c);
                    print!("*");
                }
                _ => {}
            }
            std::io::stdout().flush()?;
        }
    }
}

fn settings_lock(path: &Path, private_home: bool) -> Result<File> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent).context("create private settings directory")?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW).bits() as i32)
        .open(parent)
        .context("open settings directory without following a symlink")?;
    let meta = directory.metadata()?;
    ensure!(
        meta.uid() == rustix::process::getuid().as_raw(),
        "settings directory must belong to the current user"
    );
    if private_home {
        // Repair only our owned default directory. Use its opened descriptor
        // so a path replacement cannot redirect the permission change.
        directory
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .context("secure the private home settings directory")?;
    } else {
        ensure!(
            meta.mode() & 0o022 == 0,
            "custom settings directory allows group/other writes; remove those write permissions before retrying"
        );
    }
    let name = path
        .file_name()
        .context("settings path needs a file name")?;
    let mut lock_name = name.to_os_string();
    lock_name.push(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(parent.join(lock_name))
        .context("open settings lock")?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file()
            && meta.uid() == rustix::process::getuid().as_raw()
            && meta.mode() & 0o077 == 0,
        "settings lock must be a private regular file owned by the current user"
    );
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .context("another setup is updating these settings; finish it before retrying")?;
    Ok(file)
}

fn save(path: &Path, config: &Config) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let text = toml::to_string_pretty(config).context("encode private settings")?;
    ensure!(
        text.len() <= 64 * 1024,
        "saved settings would exceed 64 KiB"
    );
    let mut file =
        tempfile::NamedTempFile::new_in(parent).context("prepare private settings transaction")?;
    file.write_all(text.as_bytes())
        .context("write private settings")?;
    file.as_file()
        .sync_all()
        .context("synchronize private settings")?;
    file.persist(path)
        .map_err(|_| anyhow::anyhow!("commit private settings transaction"))?;
    File::open(parent)?
        .sync_all()
        .context("synchronize settings directory")?;
    Ok(())
}
