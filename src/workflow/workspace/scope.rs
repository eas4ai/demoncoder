use std::path::Path;

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Developer-selected generated files or directory subtrees, relative to the root.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureScope {
    generated_outputs: Vec<String>,
}

impl CaptureScope {
    pub fn new(mut generated_outputs: Vec<String>) -> Result<Self> {
        ensure!(
            generated_outputs.len() <= 128,
            "generated-output scope allows at most 128 paths"
        );
        generated_outputs.sort();
        generated_outputs.dedup();
        let scope = Self { generated_outputs };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.generated_outputs.len() <= 128
                && self
                    .generated_outputs
                    .iter()
                    .map(String::len)
                    .sum::<usize>()
                    <= 32 * 1024
                && self
                    .generated_outputs
                    .iter()
                    .all(|path| crate::subagents::state::valid_content_path(path))
                && self
                    .generated_outputs
                    .windows(2)
                    .all(|pair| pair[0] < pair[1]),
            "generated-output scope requires bounded relative file or subtree paths; root, parent traversal, private paths and Git administration are not allowed"
        );
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.generated_outputs.is_empty()
    }

    pub fn excludes(&self, path: &Path) -> bool {
        let path = path.strip_prefix(".").unwrap_or(path);
        self.generated_outputs
            .iter()
            .any(|output| path.starts_with(output))
    }
}
