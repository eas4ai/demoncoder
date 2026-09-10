//! Gate selection is independent of generated-output/review exclusions.
use anyhow::{Result, ensure};
use globset::{GlobBuilder, GlobMatcher};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

/// A validated declaration over the public workspace namespace. Default means
/// every admitted entry, including generated outputs, dirty and untracked bytes.
/// Literal paths select the entry and its complete subtree and must exist.
/// Globs use globset syntax, case sensitively, with literal separators: `*` does
/// not cross `/`, `**` can. A glob selects matching entries (not their subtrees);
/// a matching directory includes its immediate public membership names. Zero
/// matches is valid evidence. Explicit absent paths must not exist.
/// Protected literal paths/anchors are refused, even when absent. Protected
/// entries are never read; an explicit match or selected directory requiring one
/// is refused. Broad glob traversal otherwise uses the public namespace only.
#[derive(Clone, Default, Serialize)]
pub struct GateReadSet {
    explicit: bool,
    paths: Vec<String>,
    globs: Vec<String>,
    absent: Vec<String>,
    #[serde(skip)]
    compiled: Vec<GlobMatcher>,
    #[serde(skip)]
    anchors: Vec<String>,
}
impl std::fmt::Debug for GateReadSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GateReadSet")
            .field("explicit", &self.explicit)
            .field("paths", &self.paths.len())
            .field("globs", &self.globs.len())
            .field("absent", &self.absent.len())
            .finish()
    }
}
impl GateReadSet {
    pub fn new(
        mut paths: Vec<String>,
        mut globs: Vec<String>,
        mut absent: Vec<String>,
    ) -> Result<Self> {
        ensure!(
            paths.len() + globs.len() + absent.len() <= 128
                && paths
                    .iter()
                    .chain(&globs)
                    .chain(&absent)
                    .map(String::len)
                    .sum::<usize>()
                    <= 32768,
            "gate read set exceeds declaration bounds"
        );
        for list in [&mut paths, &mut globs, &mut absent] {
            list.sort();
            list.dedup();
        }
        for path in paths.iter().chain(&absent) {
            validate_path(path)?;
        }
        let mut compiled = Vec::new();
        let mut anchors = Vec::new();
        for pattern in &globs {
            validate_path(pattern)?;
            let anchor = pattern
                .split('/')
                .take_while(|part| !part.contains(['*', '?', '[', '{']))
                .collect::<Vec<_>>()
                .join("/");
            if !anchor.is_empty() {
                validate_path(&anchor)?;
            }
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map_err(|_| anyhow::anyhow!("invalid gate glob"))?;
            compiled.push(glob.compile_matcher());
            anchors.push(anchor);
        }
        ensure!(
            !paths
                .iter()
                .any(|path| absent.iter().any(|missing| beneath(path, missing))),
            "a required path cannot be inside an explicitly absent path"
        );
        Ok(Self {
            explicit: true,
            paths,
            globs,
            absent,
            compiled,
            anchors,
        })
    }
    pub fn absent_paths(&self) -> &[String] {
        &self.absent
    }
    pub(crate) fn selects(&self, path: &Path) -> bool {
        let name = relative(path);
        !self.explicit
            || self.paths.iter().any(|p| beneath(name, p))
            || self.compiled.iter().any(|glob| glob.is_match(name))
    }
    pub(crate) fn visits(&self, path: &Path) -> bool {
        let name = relative(path);
        self.selects(path)
            || self.ancestor(path)
            || self
                .anchors
                .iter()
                .any(|anchor| anchor.is_empty() || beneath(name, anchor) || beneath(anchor, name))
    }
    pub(crate) fn ancestor(&self, path: &Path) -> bool {
        let name = relative(path);
        name.is_empty()
            || self
                .paths
                .iter()
                .chain(&self.absent)
                .chain(&self.anchors)
                .any(|target| beneath(target, name))
    }
    pub(crate) fn requires_protected(&self, parent: &Path, child: &Path) -> bool {
        self.explicit && (self.selects(parent) || self.selects(child) || self.ancestor(child))
    }
    pub(crate) fn matches(
        &self,
        entries: &BTreeMap<String, super::Entry>,
        checkpoint: &dyn Fn() -> Result<()>,
    ) -> Result<BTreeMap<String, Vec<String>>> {
        let mut matches = BTreeMap::new();
        let mut bytes = 0usize;
        for (pattern, matcher) in self.globs.iter().zip(&self.compiled) {
            let mut names = Vec::new();
            bytes += pattern.len();
            for name in entries.keys() {
                checkpoint()?;
                let name = relative(Path::new(name));
                if matcher.is_match(name) {
                    bytes += name.len() + std::mem::size_of::<String>();
                    ensure!(
                        bytes <= super::MAX_METADATA_BYTES,
                        "gate glob membership evidence exceeds 8 MiB"
                    );
                    names.push(name.to_owned());
                }
            }
            matches.insert(pattern.clone(), names);
        }
        Ok(matches)
    }
    pub(crate) fn finish(&self, entries: &mut BTreeMap<String, super::Entry>) -> Result<()> {
        for required in &self.paths {
            ensure!(
                entries.contains_key(&format!("./{required}")),
                "required gate path is missing or unreachable"
            );
        }
        for absent in &self.absent {
            ensure!(
                !entries.contains_key(&format!("./{absent}")),
                "required absent gate path now exists"
            );
        }
        // An absent descendant of a symlink/file is not evidence of an absent
        // entry in a directory. Never resolve it through a live target.
        for target in self.paths.iter().chain(&self.absent).chain(&self.anchors) {
            for ancestor in Path::new(target)
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
            {
                if let Some(entry) = entries.get(&format!("./{}", ancestor.display())) {
                    ensure!(
                        entry.kind == super::Kind::Directory,
                        "gate path has a non-directory ancestor"
                    );
                }
            }
        }
        for (anchor, pattern) in self.anchors.iter().zip(&self.globs) {
            if !anchor.is_empty()
                && anchor != pattern
                && let Some(entry) = entries.get(&format!("./{anchor}"))
            {
                ensure!(
                    entry.kind == super::Kind::Directory,
                    "gate glob has a non-directory anchor"
                );
            }
        }
        let mut retained = std::collections::BTreeSet::new();
        for name in entries.keys().filter(|name| self.selects(Path::new(name))) {
            for ancestor in Path::new(name).ancestors() {
                retained.insert(ancestor.to_owned());
            }
        }
        entries
            .retain(|name, _| self.ancestor(Path::new(name)) || retained.contains(Path::new(name)));
        Ok(())
    }
}
fn relative(path: &Path) -> &str {
    path.strip_prefix(".")
        .unwrap_or(path)
        .to_str()
        .unwrap_or("")
}
fn beneath(path: &str, parent: &str) -> bool {
    Path::new(path).starts_with(parent)
}
fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && path.len() <= 4096
            && !path.contains(['\\', '\0'])
            && path.split('/').count() <= 64
            && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
            && !path.split('/').any(|part| part == ".git")
            && !crate::export_policy::private_path(Path::new(path)),
        "gate selection requires a bounded canonical public workspace-relative path"
    );
    Ok(())
}
