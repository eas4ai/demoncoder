//! Bounded status reads for the selected workspace, away from the input loop.
use crate::events::ContextUsage;
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::mpsc,
    task::JoinHandle,
};

const MAX_GIT_BYTES: usize = 256 * 1024;

#[derive(Clone, Default)]
pub struct DisplayOptions {
    pub model: Option<String>,
    pub workspace: Option<PathBuf>,
    pub context_window: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct GitStatus {
    pub branch: String,
    pub dirty: usize,
    pub diff: Option<(u64, u64)>,
}

pub(crate) struct GitPoller(Option<JoinHandle<()>>);
impl GitPoller {
    pub(crate) fn start(
        workspace: Option<PathBuf>,
        updates: mpsc::Sender<Result<GitStatus, String>>,
    ) -> Self {
        Self(workspace.map(|workspace| {
            tokio::spawn(async move {
                loop {
                    let status =
                        tokio::time::timeout(Duration::from_secs(2), collect_git(&workspace))
                            .await
                            .map_err(|_| "Git status timed out".to_owned())
                            .and_then(|r| r.map_err(|e| e.to_string()));
                    if updates.send(status).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            })
        }))
    }
}
impl Drop for GitPoller {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

async fn limited(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_GIT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(bytes.len() <= MAX_GIT_BYTES, "Git output exceeds 256 KiB");
    Ok(bytes)
}

async fn git(workspace: &Path, overrides: &[String], args: &[&str]) -> Result<(bool, Vec<u8>)> {
    let mut child = Command::new("git")
        .current_dir(workspace)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
        ])
        .args(overrides)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Git is unavailable")?;
    let stdout = child.stdout.take().context("Git stdout unavailable")?;
    let stderr = child.stderr.take().context("Git stderr unavailable")?;
    let (output, _, exit) = tokio::try_join!(limited(stdout), limited(stderr), async {
        Ok::<_, anyhow::Error>(child.wait().await?)
    })?;
    Ok((exit.success(), output))
}

async fn collect_git(workspace: &Path) -> Result<GitStatus> {
    // Disable configured content filters before any operation that might inspect
    // a working-tree file. Passive status must not launch project filter commands.
    let (_, filters) = git(
        workspace,
        &[],
        &[
            "config",
            "--null",
            "--get-regexp",
            "^filter\\..*\\.(clean|smudge|process)$",
        ],
    )
    .await?;
    let mut overrides = Vec::new();
    for record in filters.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let key = record
            .split(|b| *b == b'\n')
            .next()
            .context("invalid Git filter key")?;
        let key = std::str::from_utf8(key).context("invalid Git filter key")?;
        overrides.extend(["-c".into(), format!("{key}=")]);
    }
    let (ok, bytes) = git(
        workspace,
        &overrides,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--branch",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    )
    .await?;
    ensure!(
        ok,
        "Git status unavailable (not a repository or command failed)"
    );
    let mut records = bytes.split(|b| *b == 0).filter(|r| !r.is_empty());
    let header = records.next().context("Git branch unavailable")?;
    ensure!(header.starts_with(b"## "), "Git branch unavailable");
    let header = String::from_utf8_lossy(&header[3..]);
    let branch = header
        .strip_prefix("No commits yet on ")
        .or_else(|| header.strip_prefix("Initial commit on "))
        .unwrap_or(&header)
        .split("...")
        .next()
        .unwrap_or("?")
        .to_owned();
    let mut dirty = 0;
    while let Some(record) = records.next() {
        ensure!(record.len() >= 3, "invalid Git status record");
        dirty += 1;
        if record[..2].iter().any(|b| *b == b'R' || *b == b'C') {
            records.next().context("missing Git rename path")?;
        }
    }
    let (ok, diff) = git(
        workspace,
        &overrides,
        &[
            "diff",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--ignore-submodules=all",
            "HEAD",
            "--",
        ],
    )
    .await?;
    let diff = if ok { Some(parse_diff(&diff)?) } else { None };
    Ok(GitStatus {
        branch,
        dirty,
        diff,
    })
}

fn parse_diff(bytes: &[u8]) -> Result<(u64, u64)> {
    let mut sums = (0u64, 0u64);
    for row in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let mut columns = row.splitn(3, |b| *b == b'\t');
        let mut count = || -> Result<u64> {
            let field = columns.next().context("invalid Git diff count")?;
            if field == b"-" {
                return Ok(0);
            } // Binary files have no line counts.
            Ok(std::str::from_utf8(field)?.parse()?)
        };
        sums.0 = sums
            .0
            .checked_add(count()?)
            .context("Git additions overflow")?;
        sums.1 = sums
            .1
            .checked_add(count()?)
            .context("Git deletions overflow")?;
        ensure!(columns.next().is_some(), "Git diff path unavailable");
    }
    Ok(sums)
}

pub(crate) fn context_text(context: ContextUsage, override_capacity: Option<u64>) -> String {
    let capacity = override_capacity.or(context.capacity).filter(|n| *n > 0);
    let approximate = if context.estimated { "~" } else { "" };
    let used = context
        .used
        .map_or_else(|| "?".into(), |n| format!("{approximate}{n}"));
    let total = capacity.map_or_else(|| "?".into(), |n| n.to_string());
    let free = context.used.zip(capacity).map_or_else(
        || "?".into(),
        |(used, cap)| format!("{approximate}{}", cap.saturating_sub(used)),
    );
    format!("Ctx {used}/{total} · {free} free")
}

pub(crate) fn usage_text(
    input: Option<u64>,
    output: Option<u64>,
    cached: Option<u64>,
    cost: Option<f64>,
) -> String {
    let cost = cost.filter(|n| n.is_finite() && *n >= 0.0);
    if input.is_none() && output.is_none() && cached.is_none() && cost.is_none() {
        return String::new();
    }
    let count = |n: Option<u64>| n.map_or_else(|| "unknown".into(), |n| n.to_string());
    let mut text = format!(
        "in {} · out {} · cached {}",
        count(input),
        count(output),
        count(cached)
    );
    if let Some(cost) = cost {
        text.push_str(&format!(" · cost ${cost:.4}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_context_and_cost_keep_unknown_estimates_and_zero_distinct() {
        assert_eq!(
            context_text(
                ContextUsage {
                    used: Some(80),
                    capacity: Some(100),
                    estimated: true
                },
                None
            ),
            "Ctx ~80/100 · ~20 free"
        );
        assert_eq!(
            context_text(ContextUsage::default(), Some(100)),
            "Ctx ?/100 · ? free"
        );
        assert_eq!(
            context_text(
                ContextUsage {
                    used: Some(u64::MAX),
                    capacity: Some(1),
                    estimated: false
                },
                None
            ),
            format!("Ctx {}/1 · 0 free", u64::MAX)
        );
        assert!(!usage_text(Some(0), None, None, None).contains("cost"));
        assert!(usage_text(None, None, None, Some(0.0)).contains("cost $0.0000"));
        assert!(usage_text(None, None, None, Some(f64::NAN)).is_empty());
    }
    #[tokio::test]
    async fn status_output_and_diff_parsing_are_bounded_and_nul_delimited() {
        assert_eq!(
            parse_diff(b"1\t2\tname\nwith\ttabs\0-\t-\tbinary\0").unwrap(),
            (1, 2)
        );
        assert!(parse_diff(b"bad\t0\tx\0").is_err());
        assert!(
            limited(std::io::Cursor::new(vec![b'x'; MAX_GIT_BYTES + 1]))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn status_uses_actual_workspace_and_never_runs_configured_filters() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path();
        let command = |args: &[&str]| {
            let result = std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .output()
                .unwrap();
            assert!(result.status.success(), "{:?}", result.stderr);
        };
        command(&["init", "-q", "-b", "status-test"]);
        assert_eq!(
            collect_git(dir).await.unwrap().diff,
            None,
            "unborn HEAD must stay unknown"
        );
        std::fs::write(dir.join("file"), "old\n").unwrap();
        command(&["add", "file"]);
        command(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=f@invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "seed",
        ]);
        std::fs::write(dir.join("file"), "new\n").unwrap();
        std::fs::write(dir.join("untracked\nname"), "x").unwrap();
        std::fs::write(dir.join(".gitattributes"), "file filter=probe\n").unwrap();
        command(&["config", "filter.probe.clean", "touch FILTER-RAN; cat"]);
        let status = collect_git(dir).await.unwrap();
        assert_eq!(status.branch, "status-test");
        assert_eq!(status.dirty, 3);
        assert_eq!(status.diff, Some((1, 1)));
        assert!(
            !dir.join("FILTER-RAN").exists(),
            "passive status executed a filter"
        );
        let other = tempfile::tempdir().unwrap();
        assert!(collect_git(other.path()).await.is_err());
    }
}
