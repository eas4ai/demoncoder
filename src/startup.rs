//! Interactive setup writes one private settings transaction before session startup.
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, IsTerminal, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::config::{Args, Config};

pub async fn prepare(args: &Args) -> Result<()> {
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
            "project writes -- normal reads, network, and installed tools; private credentials protected"
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
        crate::settings::onboarding(&mut config).await?;
        if let Some(name) = &config.default_connection
            && let Some(connection) = config.connections.get_mut(name)
        {
            if args.effort.is_some() {
                connection.effort = args.effort.clone();
            }
            if args.max_output_tokens.is_some() {
                connection.max_output_tokens = args.max_output_tokens;
            }
            connection.validate()?;
            if let Some(assignment) = config.settings.as_mut().and_then(|s| s.creator.as_mut()) {
                assignment.effort = connection.effort.clone();
            }
        }
    }
    save(&path, &config)?;
    println!("Private settings saved. Starting the selected session.\n");
    Ok(())
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

pub(crate) fn settings_lock(path: &Path, private_home: bool) -> Result<File> {
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

#[derive(Debug)]
pub(crate) struct PublicationUncertain;
impl std::fmt::Display for PublicationUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("settings were published but directory synchronization failed; inspect the saved revision before continuing")
    }
}
impl std::error::Error for PublicationUncertain {}

pub(crate) fn save(path: &Path, config: &Config) -> Result<()> {
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
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| PublicationUncertain)?;
    Ok(())
}
