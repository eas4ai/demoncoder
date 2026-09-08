use std::path::Path;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Developer-selected output exclusions and supporting review context.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CaptureScope {
    generated_outputs: Vec<String>,
    /// None retains the whole baseline; an empty selection reviews changes only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_context: Option<Vec<String>>,
}

impl CaptureScope {
    pub fn new(generated_outputs: Vec<String>) -> Result<Self> {
        let scope = Self {
            generated_outputs: normalize_paths(generated_outputs, "generated-output")?,
            review_context: None,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn with_review_context(mut self, paths: Option<Vec<String>>) -> Result<Self> {
        self.review_context = paths
            .map(|paths| normalize_paths(paths, "review context"))
            .transpose()?;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<()> {
        validate_paths(&self.generated_outputs, "generated-output")?;
        if let Some(paths) = &self.review_context {
            validate_paths(paths, "review context")?;
            ensure!(
                paths.iter().all(|path| !self.excludes(Path::new(path))),
                "selected review context cannot be a declared generated output"
            );
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.generated_outputs.is_empty() && self.review_context.is_none()
    }

    pub(crate) fn generated_outputs(&self) -> &[String] {
        &self.generated_outputs
    }

    pub(crate) fn review_context(&self) -> Option<&[String]> {
        self.review_context.as_deref()
    }

    pub(crate) fn includes_review_context(&self, path: &Path) -> bool {
        let path = path.strip_prefix(".").unwrap_or(path);
        self.review_context
            .as_ref()
            .is_none_or(|paths| paths.iter().any(|context| path.starts_with(context)))
    }

    pub fn excludes(&self, path: &Path) -> bool {
        let path = path.strip_prefix(".").unwrap_or(path);
        self.generated_outputs
            .iter()
            .any(|output| path.starts_with(output))
    }
}

fn normalize_paths(mut paths: Vec<String>, label: &str) -> Result<Vec<String>> {
    ensure!(paths.len() <= 128, "{label} scope allows at most 128 paths");
    paths.sort();
    paths.dedup();
    validate_paths(&paths, label)?;
    Ok(paths)
}

fn validate_paths(paths: &[String], label: &str) -> Result<()> {
    ensure!(
        paths.len() <= 128
            && paths.iter().map(String::len).sum::<usize>() <= 32 * 1024
            && paths
                .iter()
                .all(|path| crate::subagents::state::valid_content_path(path))
            && paths.windows(2).all(|pair| pair[0] < pair[1]),
        "{label} scope requires bounded relative file or subtree paths; root, parent traversal, private paths and Git administration are not allowed"
    );
    Ok(())
}
