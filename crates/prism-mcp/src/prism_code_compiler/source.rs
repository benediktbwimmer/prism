use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use super::input::PrismCodeEntryPoint;
use super::PrismCodeCompilerInput;

const PRISM_CODE_ROOT: &str = ".prism/code";
const PRISM_VIRTUAL_INLINE_SPECIFIER: &str = "file:///prism/inline/prism_code.ts";
const PRISM_VIRTUAL_CODE_ROOT_SPECIFIER: &str = "file:///prism/code";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrismCodeSourceOrigin {
    InlineSnippet,
    RepoModule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrismCodeSourceUnit {
    origin: PrismCodeSourceOrigin,
    specifier: String,
    display_path: String,
    source_text: String,
}

impl PrismCodeSourceUnit {
    pub(crate) fn origin(&self) -> PrismCodeSourceOrigin {
        self.origin
    }

    pub(crate) fn specifier(&self) -> &str {
        &self.specifier
    }

    pub(crate) fn display_path(&self) -> &str {
        &self.display_path
    }

    pub(crate) fn source_text(&self) -> &str {
        &self.source_text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrismCodeSourceBundle {
    root: PrismCodeSourceUnit,
}

impl PrismCodeSourceBundle {
    pub(crate) fn root(&self) -> &PrismCodeSourceUnit {
        &self.root
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrismCodeResolvedImport {
    RepoModule(PathBuf),
    RuntimeModule(String),
}

pub(crate) fn load_compiler_sources(
    input: &PrismCodeCompilerInput,
    workspace_root: Option<&Path>,
) -> Result<PrismCodeSourceBundle> {
    match input.entry_point() {
        PrismCodeEntryPoint::InlineSnippet { code, .. } => Ok(PrismCodeSourceBundle {
            root: PrismCodeSourceUnit {
                origin: PrismCodeSourceOrigin::InlineSnippet,
                specifier: PRISM_VIRTUAL_INLINE_SPECIFIER.to_string(),
                display_path: "inline:prism_code.ts".to_string(),
                source_text: code.clone(),
            },
        }),
        PrismCodeEntryPoint::RepoModule(module) => {
            let workspace_root = workspace_root
                .context("repo-authored prism_code modules require a workspace root")?;
            let repo_relative = normalize_repo_module_path(&module.module_path)?;
            let full_path = workspace_root.join(PRISM_CODE_ROOT).join(&repo_relative);
            let source_text = fs::read_to_string(&full_path).with_context(|| {
                format!(
                    "failed to read prism_code module `{}`",
                    repo_relative.display()
                )
            })?;
            Ok(PrismCodeSourceBundle {
                root: PrismCodeSourceUnit {
                    origin: PrismCodeSourceOrigin::RepoModule,
                    specifier: repo_module_specifier(&repo_relative),
                    display_path: format!("{}/{}", PRISM_CODE_ROOT, repo_relative.display()),
                    source_text,
                },
            })
        }
    }
}

pub(crate) fn resolve_repo_module_import(
    base_module_path: &Path,
    import_specifier: &str,
) -> Result<PrismCodeResolvedImport> {
    if import_specifier.starts_with("prism:") {
        return Ok(PrismCodeResolvedImport::RuntimeModule(
            import_specifier.to_string(),
        ));
    }
    if !import_specifier.starts_with("./") && !import_specifier.starts_with("../") {
        bail!(
            "unsupported prism_code import `{import_specifier}`; use a relative `.prism/code/**` import or an approved `prism:` runtime module"
        );
    }
    let base_module_path = normalize_repo_module_path(base_module_path)?;
    let base_dir = base_module_path.parent().ok_or_else(|| {
        anyhow!(
            "repo module `{}` has no parent directory",
            base_module_path.display()
        )
    })?;
    let candidate = normalize_relative_module_path(base_dir.join(import_specifier))?;
    Ok(PrismCodeResolvedImport::RepoModule(candidate))
}

fn repo_module_specifier(path: &Path) -> String {
    format!(
        "{}/{}",
        PRISM_VIRTUAL_CODE_ROOT_SPECIFIER,
        path.to_string_lossy().replace('\\', "/")
    )
}

fn normalize_repo_module_path(path: &Path) -> Result<PathBuf> {
    let stripped = if path.starts_with(PRISM_CODE_ROOT) {
        path.strip_prefix(PRISM_CODE_ROOT)?
    } else {
        path
    };
    normalize_relative_module_path(stripped)
}

fn normalize_relative_module_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!(
                        "prism_code module path `{}` escapes `.prism/code`",
                        path.as_ref().display()
                    );
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!(
                    "prism_code module path `{}` must be relative to `.prism/code`",
                    path.as_ref().display()
                );
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        bail!("prism_code module path cannot be empty");
    }

    if normalized.extension().is_none() {
        normalized.set_extension("ts");
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::QueryLanguage;

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "prism_code_compiler_tests_{}_{}",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn inline_source_bundle_uses_virtual_specifier() {
        let input = PrismCodeCompilerInput::inline(
            "prism_code",
            "return prism.runtimeStatus();",
            QueryLanguage::Ts,
            false,
        );
        let bundle = load_compiler_sources(&input, None).expect("inline source should load");
        assert_eq!(bundle.root().origin(), PrismCodeSourceOrigin::InlineSnippet);
        assert_eq!(bundle.root().specifier(), PRISM_VIRTUAL_INLINE_SPECIFIER);
        assert_eq!(bundle.root().display_path(), "inline:prism_code.ts");
    }

    #[test]
    fn repo_module_source_bundle_reads_under_prism_code_root() {
        let root = unique_temp_dir();
        let module_dir = root.join(PRISM_CODE_ROOT).join("plans");
        fs::create_dir_all(&module_dir).expect("module directory should create");
        let module_path = module_dir.join("deploy.ts");
        fs::write(&module_path, "export default async function deploy() {}\n")
            .expect("module should write");

        let input = PrismCodeCompilerInput::repo_module_compile(
            "prism_code",
            "plans/deploy.ts",
            Some("default".to_string()),
            QueryLanguage::Ts,
        );
        let bundle =
            load_compiler_sources(&input, Some(&root)).expect("repo module source should load");
        assert_eq!(bundle.root().origin(), PrismCodeSourceOrigin::RepoModule);
        assert_eq!(bundle.root().display_path(), ".prism/code/plans/deploy.ts");
        assert_eq!(
            bundle.root().specifier(),
            "file:///prism/code/plans/deploy.ts"
        );

        fs::remove_dir_all(&root).expect("temporary test directory should remove");
    }

    #[test]
    fn repo_module_import_resolution_supports_relative_and_runtime_imports() {
        let resolved =
            resolve_repo_module_import(Path::new("plans/deploy.ts"), "../libraries/shared")
                .expect("relative import should resolve");
        assert_eq!(
            resolved,
            PrismCodeResolvedImport::RepoModule(PathBuf::from("libraries/shared.ts"))
        );

        let runtime_import =
            resolve_repo_module_import(Path::new("plans/deploy.ts"), "prism:stdlib")
                .expect("runtime import should resolve");
        assert_eq!(
            runtime_import,
            PrismCodeResolvedImport::RuntimeModule("prism:stdlib".to_string())
        );
    }

    #[test]
    fn repo_module_import_resolution_rejects_escape() {
        let error = resolve_repo_module_import(Path::new("plans/deploy.ts"), "../../../etc/passwd")
            .expect_err("escaping import should fail");
        assert!(error.to_string().contains("escapes `.prism/code`"));
    }
}
