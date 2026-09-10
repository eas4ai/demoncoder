//! Read-only import records. Import validity is not an execution or activation receipt.
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Claude,
    Codex,
    Portable,
}
#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    pub dialect: Option<Dialect>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceValidity {
    Valid,
    Invalid,
    Unvalidated,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutableReadiness {
    Unavailable,
    Invalid,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComponentKind {
    Skill,
    Command,
    Agent,
    Workflow,
    Hooks,
    Mcp,
    Lsp,
    OutputStyle,
    Theme,
    Monitor,
    App,
    Configuration,
    Unknown,
}
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub component: Option<String>,
    pub field: String,
    pub message: String,
}
#[derive(Clone, Debug)]
pub struct Component {
    pub identity: String,
    pub kind: ComponentKind,
    pub declaration: String,
    pub required: bool,
    pub validity: SourceValidity,
    pub readiness: ExecutableReadiness,
    pub metadata: Value,
}
#[derive(Clone, Debug, Default)]
pub struct InspectionReport {
    pub diagnostics: Vec<Diagnostic>,
    pub exclusions: Vec<String>,
    pub composition: Vec<String>,
}
impl InspectionReport {
    pub fn executable_ready(&self) -> bool {
        false
    }
}
#[derive(Clone, Debug)]
pub struct SnapshotFile {
    pub(super) bytes: Vec<u8>,
    pub(super) mode: u32,
    pub(super) digest: String,
}
impl SnapshotFile {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn mode(&self) -> u32 {
        self.mode
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
}
/// A captured directory, including empty directories that affect discovery.
#[derive(Clone, Debug)]
pub struct SnapshotDirectory {
    pub(super) mode: u32,
}
impl SnapshotDirectory {
    pub fn mode(&self) -> u32 {
        self.mode
    }
}
#[derive(Clone, Debug)]
pub struct SourceIdentity {
    pub canonical_root: PathBuf,
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, Debug)]
pub struct Package {
    pub(super) source: SourceIdentity,
    pub(super) digest: String,
    pub(super) files: BTreeMap<String, SnapshotFile>,
    pub(super) directories: BTreeMap<String, SnapshotDirectory>,
    pub(super) dialect: Dialect,
    pub(super) name: String,
    pub(super) metadata: Value,
    pub(super) components: Vec<Component>,
    pub(super) report: InspectionReport,
}
impl Package {
    pub fn source(&self) -> &SourceIdentity {
        &self.source
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn files(&self) -> &BTreeMap<String, SnapshotFile> {
        &self.files
    }
    pub fn directories(&self) -> &BTreeMap<String, SnapshotDirectory> {
        &self.directories
    }
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn metadata(&self) -> &Value {
        &self.metadata
    }
    pub fn components(&self) -> &[Component] {
        &self.components
    }
    pub fn report(&self) -> &InspectionReport {
        &self.report
    }
    pub fn is_active(&self) -> bool {
        false
    }
    pub fn source_validity(&self) -> SourceValidity {
        if self
            .components
            .iter()
            .any(|c| c.validity == SourceValidity::Invalid)
        {
            SourceValidity::Invalid
        } else if self
            .components
            .iter()
            .any(|c| c.validity == SourceValidity::Unvalidated)
        {
            SourceValidity::Unvalidated
        } else {
            SourceValidity::Valid
        }
    }
}
