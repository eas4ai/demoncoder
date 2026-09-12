//! The hook view reuses worktree path checks, masks and the sealed socket filter.
use super::*;
use std::collections::BTreeMap;

pub(crate) struct HookView<'a> {
    pub(crate) live: &'a File,
    pub(crate) snapshot: &'a File,
    pub(crate) code: &'a File,
    pub(crate) workspace: &'a Path,
    pub(crate) writes: &'a [String],
    pub(crate) cwd: &'a Path,
    pub(crate) argv: &'a [String],
    pub(crate) environment: &'a BTreeMap<String, String>,
}
pub(crate) struct HookCommand {
    pub(crate) arguments: Vec<String>,
    _pins: Vec<File>,
}
impl HookCommand {
    pub(crate) fn launch_arguments(&self, status: &File, gate: &File) -> Vec<String> {
        let mut arguments = vec![
            "--noprofile".into(),
            "--norc".into(),
            "-c".into(),
            TRAMPOLINE.into(),
            "demoncoder-hook".into(),
        ];
        arguments.extend(
            [status, gate]
                .map(|file| format!("/proc/{}/fd/{}", std::process::id(), file.as_raw_fd())),
        );
        arguments.extend(self.arguments.clone());
        arguments
    }
}

// Source values are positional arguments throughout. The only shell text here is
// fixed host code; package event JSON never enters this program or its arguments.
const TRAMPOLINE: &str = r#"exec 6>"$1" || exit
exec 7<"$2" || exit
shift 2
exec 3<"$1" || exit
exec 4<"$2" || exit
exec 5<"$3" || exit
shift 3
task_count=$1
shift
task_keep=(3 4 5 6 7)
task_binds=()
for ((task_index=0; task_index<task_count; task_index++)); do
    exec {task_open}<"$1" || exit
    task_keep+=("$task_open")
    task_binds+=(--bind-fd "$task_open" "$2")
    shift 2
done
task_before=()
while [[ "$1" != --hook-bindings ]]; do
    [[ $# -gt 0 ]] || exit 1
    task_before+=("$1")
    shift
done
shift
for task_fd_path in /proc/self/fd/*; do
    task_fd=${task_fd_path##*/}
    case "$task_fd" in
        0|1|2) continue ;;
    esac
    task_retain=false
    for task_allowed in "${task_keep[@]}"; do
        [[ "$task_fd" != "$task_allowed" ]] || task_retain=true
    done
    if [[ "$task_retain" == false ]]; then
        [[ "$task_fd" =~ ^[0-9]+$ ]] || exit 1
        exec {task_fd}>&- || exit
    fi
done
exec "${task_before[@]}" "${task_binds[@]}" "$@""#;

const LAUNCH_GATE: &str = r#"IFS= read -r -n1 -u7 task_launch && [[ "$task_launch" == 1 ]] || exit 125
exec 7<&-
exec "$@""#;

impl WorktreeAccess {
    pub(crate) fn hook_command(
        &self,
        view: HookView<'_>,
        cancelled: &AtomicBool,
    ) -> Result<HookCommand> {
        let HookView {
            live,
            snapshot,
            code,
            workspace,
            writes,
            cwd,
            argv,
            environment,
        } = view;
        ensure!(!cancelled.load(Ordering::Relaxed), "hook launch cancelled");
        ensure!(
            std::fs::read_link(format!("/proc/self/fd/{}", live.as_raw_fd()))? == workspace,
            "hook workspace moved; reopen it"
        );
        for reserved in
            RUNTIME_PATHS
                .iter()
                .copied()
                .chain(["/proc", "/dev", "/__demoncoder_hook_code"])
        {
            let reserved = Path::new(reserved);
            ensure!(
                !workspace.starts_with(reserved) && !reserved.starts_with(workspace),
                "hook workspace overlaps its confined runtime"
            );
        }
        let descriptor =
            |file: &File| format!("/proc/{}/fd/{}", std::process::id(), file.as_raw_fd());
        let mut args = vec![
            descriptor(&self.socket_filter),
            descriptor(snapshot),
            descriptor(code),
            writes.len().to_string(),
        ];
        let mut pins = Vec::new();
        let mut masks = Vec::new();
        if !writes.is_empty() {
            let live_masks = self.inspect(
                live,
                workspace,
                cancelled,
                crate::export_policy::private_path,
            )?;
            for name in writes {
                let path = workspace.join(name);
                self.check_path(&path)?;
                let file: File = openat2(
                    live,
                    name.as_str(),
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
                )?
                .into();
                let metadata = file.metadata()?;
                ensure!(
                    metadata.is_dir() || (metadata.is_file() && metadata.nlink() == 1),
                    "hook write grant requires a directory or unlinked regular file"
                );
                args.extend([
                    descriptor(&file),
                    path.to_str().context("hook paths require UTF-8")?.into(),
                ]);
                pins.push(file);
                masks.extend(
                    live_masks
                        .iter()
                        .filter(|(mask, _)| mask.starts_with(&path))
                        .cloned(),
                );
            }
        }
        // Custom credential names are not discoverable from the snapshot's static
        // export policy. Mask the actual host exclusions in this logical view too.
        for private in &self.credentials {
            if let Ok(relative) = private.strip_prefix(workspace) {
                match openat2(
                    snapshot,
                    relative,
                    OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
                ) {
                    Ok(fd) => {
                        let metadata = File::from(fd).metadata()?;
                        ensure!(
                            !metadata.is_symlink(),
                            "hook credential exclusion is a symlink"
                        );
                        masks.push((private.clone(), metadata.is_dir()));
                    }
                    Err(rustix::io::Errno::NOENT) => {}
                    Err(error) => return Err(error).context("inspect hook credential exclusion"),
                }
            }
        }
        args.extend(
            [
                "/usr/bin/bwrap",
                "--seccomp",
                "3",
                "--info-fd",
                "6",
                "--unshare-all",
                "--die-with-parent",
                "--new-session",
            ]
            .map(String::from),
        );
        for &path in RUNTIME_PATHS {
            if Path::new(path).exists() {
                args.extend(["--ro-bind", path, path].map(String::from));
            }
        }
        args.extend(
            [
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--dir",
                "/tmp",
                "--ro-bind-fd",
                "4",
            ]
            .map(String::from),
        );
        args.push(
            workspace
                .to_str()
                .context("hook workspace requires UTF-8")?
                .into(),
        );
        args.extend(
            [
                "--ro-bind-fd",
                "5",
                "/__demoncoder_hook_code",
                "--hook-bindings",
            ]
            .map(String::from),
        );
        masks.sort();
        masks.dedup();
        for (path, directory) in masks {
            let path = path.to_str().context("hook exclusion requires UTF-8")?;
            if directory {
                args.extend(["--tmpfs", path, "--remount-ro", path].map(String::from));
            } else {
                args.extend([
                    "--ro-bind".into(),
                    self.denied
                        .path()
                        .to_str()
                        .context("hook mask path requires UTF-8")?
                        .into(),
                    path.into(),
                ]);
            }
        }
        args.extend(
            [
                "--remount-ro",
                "/",
                "--remount-ro",
                "/dev",
                "--remount-ro",
                "/proc",
                "--chdir",
            ]
            .map(String::from),
        );
        args.push(cwd.to_str().context("hook cwd requires UTF-8")?.into());
        args.push("--clearenv".into());
        for (key, value) in [
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "C.UTF-8"),
            ("HOME", "/nonexistent"),
            ("TMPDIR", "/tmp"),
        ] {
            args.extend(["--setenv", key, value].map(String::from));
        }
        // The trusted gate starts before package environment (including BASH_ENV)
        // can run startup code. Its sole inherited control descriptor closes
        // before the literal package argv executes.
        args.extend(
            [
                "--",
                "/bin/bash",
                "--noprofile",
                "--norc",
                "-c",
                LAUNCH_GATE,
                "demoncoder-hook-gate",
                "/usr/bin/env",
                "--",
            ]
            .map(String::from),
        );
        for (key, value) in environment {
            args.push(format!("{key}={value}"));
        }
        args.extend_from_slice(argv);
        Ok(HookCommand {
            arguments: args,
            _pins: pins,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, process::Stdio};

    #[tokio::test]
    async fn actual_bwrap_launch_gate_requires_exact_byte_and_closes_descriptor() {
        for token in [None, Some(b'x'), Some(b'1')] {
            let root = tempfile::tempdir().unwrap();
            let (reader, writer) =
                rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).unwrap();
            let mut writer = File::from(writer);
            let mut args = vec![
                "/usr/bin/bwrap".to_string(),
                "--unshare-all".into(),
                "--die-with-parent".into(),
            ];
            for &path in RUNTIME_PATHS {
                if Path::new(path).exists() {
                    args.extend(["--ro-bind", path, path].map(String::from));
                }
            }
            args.extend(
                [
                    "--proc",
                    "/proc",
                    "--dev",
                    "/dev",
                    "--bind",
                    root.path().to_str().unwrap(),
                    "/work",
                    "--clearenv",
                    "--",
                    "/bin/bash",
                    "--noprofile",
                    "--norc",
                    "-c",
                    LAUNCH_GATE,
                    "fixture",
                    "/bin/bash",
                    "-c",
                    "test ! -e /proc/self/fd/7 && printf ran > /work/payload",
                ]
                .map(String::from),
            );
            let child = tokio::process::Command::new("/bin/bash")
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    "exec 7<\"$1\" || exit; shift; exec \"$@\"",
                    "fixture",
                ])
                .arg(format!(
                    "/proc/{}/fd/{}",
                    std::process::id(),
                    reader.as_raw_fd()
                ))
                .args(args)
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            if let Some(token) = token {
                writer.write_all(&[token]).unwrap();
            }
            drop(writer);
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(3), child.wait_with_output())
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(
                output.status.success(),
                token == Some(b'1'),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(root.path().join("payload").exists(), token == Some(b'1'));
        }
    }
}
