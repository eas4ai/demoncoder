//! Read authority over retained evidence, never over the live workspace.
use super::HookHost;
use crate::{
    plugins::gate_snapshot::{EntryKind, GateSnapshot},
    tools::ToolCall,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::{Component, Path},
    sync::{Arc, Mutex},
};

/// Constructed only by the admitted host runner. There is no wire constructor.
pub struct SnapshotInspection {
    snapshot: Arc<GateSnapshot>,
    host: HookHost,
    limit: usize,
    output_limit: usize,
    remaining_bytes: Mutex<usize>,
    remaining: Mutex<u32>,
}

impl SnapshotInspection {
    pub(crate) fn new(
        snapshot: Arc<GateSnapshot>,
        host: HookHost,
        limit: usize,
        output_limit: usize,
        calls: u32,
    ) -> Self {
        Self {
            snapshot,
            host,
            limit,
            output_limit,
            remaining_bytes: Mutex::new(limit),
            remaining: Mutex::new(calls),
        }
    }

    pub(crate) fn validate_request(&self, value: &Value) -> Result<()> {
        ensure!(
            crate::plugins::wire::measure(value)? <= self.limit,
            "model hook request history exceeds input bound"
        );
        self.validate_delivery()
    }

    pub(super) fn reserve_delivery(&self, bytes: usize) -> Result<()> {
        let mut remaining = self
            .remaining_bytes
            .lock()
            .map_err(|_| anyhow::anyhow!("snapshot byte allowance lock failed"))?;
        ensure!(
            bytes <= *remaining,
            "model hook cumulative evidence input bound exhausted"
        );
        *remaining -= bytes;
        Ok(())
    }

    /// Re-run credential alias resolution before every model delivery. A retained
    /// public file can become a credential without changing its captured bytes.
    pub(crate) fn validate_delivery(&self) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let metadata = self.host.root.metadata()?;
        ensure!(
            (metadata.dev(), metadata.ino()) == self.snapshot.root_identity(),
            "snapshot workspace identity changed"
        );
        let protected = crate::plugins::gate_snapshot::protected_paths(
            &self.host.workspace,
            &self.host.credentials,
        )?;
        for (name, _) in self.snapshot.entries() {
            let path = Path::new(".").join(name);
            ensure!(
                !protected.iter().any(|private| path.starts_with(private)),
                "retained snapshot now contains a protected credential path; recapture required"
            );
        }
        Ok(())
    }

    pub(super) fn evidence(&self, content: bool) -> Result<Value> {
        self.validate_delivery()?;
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        for (path, entry) in self.snapshot.entries() {
            let value = if content {
                ensure!(
                    entry.bytes().len() <= self.limit,
                    "required snapshot content exceeds model input bound"
                );
                json!({"path":path,"kind":entry.kind(),"content":std::str::from_utf8(entry.bytes()).context("required snapshot content is not UTF-8")?})
            } else {
                json!({"path":path,"kind":entry.kind(),"bytes":entry.bytes().len()})
            };
            bytes = bytes.saturating_add(crate::plugins::wire::measure(&value)?);
            ensure!(
                bytes <= self.limit,
                "required snapshot evidence exceeds model input bound"
            );
            entries.push(value);
        }
        let value = json!({"revision":self.snapshot.revision(),"entries":entries,"memberships":self.snapshot.memberships(),"absent":self.snapshot.absent_paths(),"glob_matches":self.snapshot.glob_matches()});
        ensure!(
            crate::plugins::wire::measure(&value)? <= self.limit,
            "required snapshot evidence exceeds model input bound"
        );
        Ok(value)
    }

    pub(crate) fn definitions(&self) -> Vec<Value> {
        [
            ("snapshot_read", "Read one UTF-8 file from retained snapshot evidence. Absolute paths mean the admitted logical workspace. No live reads.", json!({"path":{"type":"string"}}), vec!["path"]),
            ("snapshot_list", "List the captured members of a selected snapshot directory. Supporting ancestors are not complete listings.", json!({"path":{"type":"string"}}), vec!["path"]),
            ("snapshot_search", "Find a literal UTF-8 string within captured regular files. A bounded complete result is returned or the inspection fails.", json!({"text":{"type":"string"}}), vec!["text"]),
        ].into_iter().map(|(name, description, properties, required)| json!({"name":name,"description":description,"input_schema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
    }

    pub(crate) fn execute(&self, call: &ToolCall) -> Result<String> {
        let mut remaining = self
            .remaining
            .lock()
            .map_err(|_| anyhow::anyhow!("snapshot inspection lock failed"))?;
        ensure!(*remaining > 0, "hook inspection allowance exhausted");
        *remaining -= 1;
        drop(remaining);
        ensure!(
            crate::plugins::wire::measure(&call.arguments)? <= 8192,
            "snapshot request exceeds bound"
        );
        self.validate_delivery()?;
        let result = match call.name.as_str() {
            "snapshot_read" => {
                let args: PathArgs = serde_json::from_value(call.arguments.clone())?;
                let path = self.resolve(&args.path)?;
                let entry = self
                    .snapshot
                    .entry(&path)
                    .context("required snapshot file is unavailable")?;
                ensure!(
                    matches!(entry.kind(), EntryKind::File),
                    "snapshot read requires a regular file"
                );
                ensure!(
                    entry.bytes().len() <= self.output_limit,
                    "required snapshot file exceeds output bound"
                );
                json!({"path":path,"content":std::str::from_utf8(entry.bytes()).context("snapshot file is not UTF-8")?})
            }
            "snapshot_list" => {
                let args: PathArgs = serde_json::from_value(call.arguments.clone())?;
                let path = self.resolve(&args.path)?;
                let members = self
                    .snapshot
                    .memberships()
                    .get(&path)
                    .context("complete directory membership was not captured")?;
                json!({"path":path,"members":members})
            }
            "snapshot_search" => {
                let args: SearchArgs = serde_json::from_value(call.arguments.clone())?;
                ensure!(
                    !args.text.is_empty() && args.text.len() <= 1024,
                    "snapshot search requires a bounded nonempty literal"
                );
                let mut matches = Vec::new();
                let mut size = 0usize;
                for (path, entry) in self.snapshot.entries() {
                    if !matches!(entry.kind(), EntryKind::File) {
                        continue;
                    }
                    let text = std::str::from_utf8(entry.bytes())
                        .context("snapshot search requires UTF-8 evidence")?;
                    for (line, text) in text.lines().enumerate() {
                        if text.contains(&args.text) {
                            ensure!(
                                text.len() <= self.output_limit,
                                "complete snapshot search exceeds output bound"
                            );
                            let value = json!({"path":path,"line":line + 1,"text":text});
                            size = size.saturating_add(crate::plugins::wire::measure(&value)?);
                            ensure!(
                                size <= self.output_limit,
                                "complete snapshot search exceeds output bound"
                            );
                            matches.push(value);
                        }
                    }
                }
                json!({"matches":matches})
            }
            _ => anyhow::bail!("only retained snapshot inspection tools are authorized"),
        };
        let mut result = result;
        result["revision"] = json!(self.snapshot.revision());
        ensure!(
            crate::plugins::wire::measure(&result)? <= self.output_limit,
            "complete snapshot result exceeds output bound"
        );
        let result = serde_json::to_string(&result)?;
        self.reserve_delivery(
            result
                .len()
                .saturating_add(crate::plugins::wire::measure(&call.arguments)?),
        )?;
        self.validate_delivery()?;
        Ok(result)
    }

    fn resolve(&self, name: &str) -> Result<String> {
        ensure!(
            !name.is_empty() && name.len() <= 4096 && !name.contains('\0'),
            "invalid snapshot path"
        );
        let mut pending = components(Path::new(name), &self.host.workspace)?;
        let mut resolved = Vec::new();
        let mut links = 0;
        while let Some(part) = pending.pop_front() {
            match part.as_str() {
                "." => continue,
                ".." => {
                    ensure!(resolved.pop().is_some(), "snapshot path escapes workspace");
                    continue;
                }
                _ => resolved.push(part),
            }
            let path = resolved.join("/");
            let entry = self
                .snapshot
                .entry(&path)
                .context("path is outside retained snapshot evidence")?;
            if matches!(entry.kind(), EntryKind::Symlink) {
                links += 1;
                ensure!(links <= 40, "snapshot symlink resolution exceeded bound");
                let target =
                    std::str::from_utf8(entry.bytes()).context("snapshot symlink is not UTF-8")?;
                ensure!(target.len() <= 4096, "snapshot symlink exceeds bound");
                resolved.pop();
                if Path::new(target).is_absolute() {
                    resolved.clear();
                }
                let mut target = components(Path::new(target), &self.host.workspace)?;
                target.append(&mut pending);
                pending = target;
            } else if !pending.is_empty() {
                ensure!(
                    matches!(entry.kind(), EntryKind::Directory),
                    "snapshot path traverses a non-directory"
                );
            }
        }
        let path = if resolved.is_empty() {
            ".".into()
        } else {
            resolved.join("/")
        };
        ensure!(
            self.snapshot.entry(&path).is_some(),
            "path was not captured"
        );
        Ok(path)
    }
}

fn components(path: &Path, workspace: &Path) -> Result<VecDeque<String>> {
    let path = if path.is_absolute() {
        path.strip_prefix(workspace)
            .context("snapshot absolute path is outside workspace")?
    } else {
        path
    };
    path.components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name
                .to_str()
                .context("snapshot path is not UTF-8")?
                .to_owned()),
            Component::CurDir => Ok(".".into()),
            Component::ParentDir => Ok("..".into()),
            _ => anyhow::bail!("invalid snapshot path component"),
        })
        .collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::gate_snapshot::{GateReadSet, GateWorkspace};
    use std::sync::atomic::AtomicBool;

    fn inspection(root: &Path, credentials: Vec<std::path::PathBuf>) -> SnapshotInspection {
        let gate = GateWorkspace::open_with_credentials(root, &credentials).unwrap();
        let snapshot = Arc::new(
            gate.capture(&GateReadSet::default(), &AtomicBool::new(false))
                .unwrap(),
        );
        SnapshotInspection::new(
            snapshot,
            HookHost::new(
                Arc::new(std::fs::File::open(root).unwrap()),
                root.into(),
                gate.frozen_credentials(),
                None,
            ),
            32768,
            16384,
            8,
        )
    }

    #[test]
    fn retained_reads_resolve_logical_absolute_and_symlink_paths_without_live_reads() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("public"), "retained").unwrap();
        std::os::unix::fs::symlink(root.path().join("public"), root.path().join("alias")).unwrap();
        let view = inspection(root.path(), vec![]);
        std::fs::write(root.path().join("public"), "live").unwrap();
        for path in [
            "public".into(),
            "alias".into(),
            root.path().join("alias").to_str().unwrap().to_owned(),
        ] {
            let result = view
                .execute(&ToolCall {
                    id: "test".into(),
                    name: "snapshot_read".into(),
                    arguments: json!({"path":path}),
                })
                .unwrap();
            let value: Value = serde_json::from_str(&result).unwrap();
            assert_eq!(value["content"], "retained");
            assert_eq!(value["path"], "public");
            assert_eq!(value["revision"], view.snapshot.revision());
        }
        assert!(view.resolve("../outside").is_err());
        assert!(view.resolve("/etc/passwd").is_err());
    }

    #[test]
    fn changed_credential_alias_holds_before_retained_bytes_are_delivered() {
        let root = tempfile::tempdir().unwrap();
        let aliases = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("old"), "private old").unwrap();
        std::fs::write(root.path().join("new"), "private canary").unwrap();
        let alias = aliases.path().join("credential");
        std::os::unix::fs::symlink(root.path().join("old"), &alias).unwrap();
        let view = inspection(root.path(), vec![alias.clone()]);
        assert!(view.resolve("old").is_err());
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(root.path().join("new"), &alias).unwrap();
        assert!(view.evidence(true).is_err());
        assert!(
            view.execute(&ToolCall {
                id: "test".into(),
                name: "snapshot_read".into(),
                arguments: json!({"path":"new"})
            })
            .is_err()
        );
    }

    #[test]
    fn listing_and_literal_search_use_bounded_retained_evidence() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("public"), "a.*b\na something b\n").unwrap();
        std::os::unix::fs::symlink("cycle", root.path().join("cycle")).unwrap();
        let view = inspection(root.path(), vec![]);
        let call = |name: &str, arguments| ToolCall {
            id: "test".into(),
            name: name.into(),
            arguments,
        };
        let list: Value = serde_json::from_str(
            &view
                .execute(&call("snapshot_list", json!({"path":"."})))
                .unwrap(),
        )
        .unwrap();
        assert!(
            list["members"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p == "public")
        );
        let found: Value = serde_json::from_str(
            &view
                .execute(&call("snapshot_search", json!({"text":"a.*b"})))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(found["matches"].as_array().unwrap().len(), 1);
        assert_eq!(found["matches"][0]["line"], 1);
        assert!(view.resolve("cycle").is_err());
        assert!(
            view.execute(&call("snapshot_search", json!({"text":""})))
                .is_err()
        );
        assert!(
            view.execute(&call("snapshot_list", json!({"path":"public"})))
                .is_err()
        );
    }
}
