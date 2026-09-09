//! Source-export exclusions, independent of Git ignore rules and model requests.
//! Explicit tool access does not authorize automatic disclosure in review or Git snapshots.
use std::path::{Component, Path, PathBuf};

pub(crate) const VERSION: u8 = 2;

/// Shared with the developer tool boundary; preserve its home-relative protection list.
pub(crate) const PRIVATE_PATHS: &[&str] = &[
    ".demoncoder",
    ".codex",
    ".claude",
    ".claude.json",
    ".ssh",
    ".aws",
    ".azure",
    ".kube",
    ".netrc",
    ".npmrc",
    ".git-credentials",
    ".config/git/credentials",
    ".config/gh",
    ".config/gcloud",
    ".config/opencode/auth.json",
    ".cargo/credentials",
    ".cargo/credentials.toml",
];

pub(crate) const DESCRIPTION: &str = "Private runtime/credential paths (.demoncoder, .codex, .claude, .ssh, .aws, .azure, .kube, credential dotfiles, .config Git/GitHub/gcloud/OpenCode credentials, Cargo credentials and .env variants) are excluded before reading contents. Public .env.example/.env.sample/.env.template files remain source. Excluded files are not reviewed or integrated. Workspaces overlapping declared private roots are refused before capture; older snapshots require a new task baseline.";

/// Keep automatic export and ordinary tool access on the same declared roots.
/// Lexical names are retained here; consumers also protect canonical aliases.
pub(crate) fn private_roots(credential_paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        paths.extend(PRIVATE_PATHS.iter().map(|path| Path::new(&home).join(path)));
    }
    for variable in [
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "AWS_SHARED_CREDENTIALS_FILE",
    ] {
        if let Some(path) = std::env::var_os(variable) {
            paths.push(PathBuf::from(path));
        }
    }
    paths.extend_from_slice(credential_paths);
    paths
}

/// Callers supply the canonical workspace root. Check both configured and physical names.
pub(crate) fn contains_declared_private(root: &Path, paths: &[PathBuf]) -> anyhow::Result<bool> {
    for path in paths {
        let lexical = std::path::absolute(path)?;
        if lexical.starts_with(root) || root.starts_with(&lexical) {
            return Ok(true);
        }
        match path.canonicalize() {
            Ok(resolved) if resolved.starts_with(root) || root.starts_with(&resolved) => {
                return Ok(true);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

pub(crate) fn private_path(path: &Path) -> bool {
    if path.ancestors().any(|ancestor| {
        PRIVATE_PATHS
            .iter()
            .any(|private| ancestor.ends_with(private))
    }) {
        return true;
    }
    path.components().any(|part| {
        let Component::Normal(name) = part else {
            return false;
        };
        let Some(name) = name.to_str() else {
            return false;
        };
        name == ".pypirc"
            || ((name == ".env" || name.starts_with(".env."))
                && !matches!(name, ".env.example" | ".env.sample" | ".env.template"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_private_paths_include_relative_names_and_symlink_aliases() {
        let cwd = std::env::current_dir().unwrap();
        let root = tempfile::tempdir_in(&cwd).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = root.path().join("custom-settings.toml");
        std::fs::write(&secret, "synthetic credential").unwrap();
        let alias = outside.path().join("settings-link");
        std::os::unix::fs::symlink(&secret, &alias).unwrap();
        for path in [
            secret.clone(),
            secret.strip_prefix(&cwd).unwrap().to_owned(),
            alias,
        ] {
            assert!(contains_declared_private(root.path(), &[path]).unwrap());
        }
        assert!(
            !contains_declared_private(root.path(), &[outside.path().join("missing")]).unwrap()
        );
    }

    #[test]
    fn nested_private_entries_are_distinct_from_public_names_and_templates() {
        for private in PRIVATE_PATHS {
            assert!(private_path(Path::new(private)), "{private}");
            assert!(
                private_path(&Path::new("nested").join(private).join("value")),
                "{private}"
            );
        }
        for name in [
            ".env",
            "a/.env.production",
            "a/.demoncoder/token",
            "a/.ssh/key",
            ".npmrc",
        ] {
            assert!(private_path(Path::new(name)), "{name}");
        }
        for name in [
            ".env.example",
            ".env.sample",
            ".env.template",
            "environment.rs",
            "a/public.env",
            "codex.rs",
            ".cargo/config.toml",
            ".config/editor/preferences.json",
        ] {
            assert!(!private_path(Path::new(name)), "{name}");
        }
    }
}
