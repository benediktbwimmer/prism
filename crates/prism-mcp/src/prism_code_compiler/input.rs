use std::path::PathBuf;

use crate::QueryLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrismCodeExecutionProduct {
    ReadOnlyEvaluation,
    TransactionalWrite,
    ReusableArtifactCompilation,
    CompileAndInstantiate,
}

impl PrismCodeExecutionProduct {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::ReadOnlyEvaluation => "read_only_evaluation",
            Self::TransactionalWrite => "transactional_write",
            Self::ReusableArtifactCompilation => "reusable_artifact_compilation",
            Self::CompileAndInstantiate => "compile_and_instantiate",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PrismCodeModuleEntryPoint {
    pub(crate) module_path: PathBuf,
    pub(crate) export_name: Option<String>,
    pub(crate) language: QueryLanguage,
}

#[derive(Debug, Clone)]
pub(crate) enum PrismCodeEntryPoint {
    InlineSnippet {
        code: String,
        language: QueryLanguage,
    },
    RepoModule(PrismCodeModuleEntryPoint),
}

#[derive(Debug, Clone)]
pub(crate) struct PrismCodeCompilerInput {
    surface_name: &'static str,
    execution_product: PrismCodeExecutionProduct,
    entry_point: PrismCodeEntryPoint,
}

impl PrismCodeCompilerInput {
    pub(crate) fn inline(
        surface_name: &'static str,
        code: impl Into<String>,
        language: QueryLanguage,
        write_capable: bool,
    ) -> Self {
        Self {
            surface_name,
            execution_product: if write_capable {
                PrismCodeExecutionProduct::TransactionalWrite
            } else {
                PrismCodeExecutionProduct::ReadOnlyEvaluation
            },
            entry_point: PrismCodeEntryPoint::InlineSnippet {
                code: code.into(),
                language,
            },
        }
    }

    pub(crate) fn repo_module_compile(
        surface_name: &'static str,
        module_path: impl Into<PathBuf>,
        export_name: Option<String>,
        language: QueryLanguage,
    ) -> Self {
        Self {
            surface_name,
            execution_product: PrismCodeExecutionProduct::ReusableArtifactCompilation,
            entry_point: PrismCodeEntryPoint::RepoModule(PrismCodeModuleEntryPoint {
                module_path: module_path.into(),
                export_name,
                language,
            }),
        }
    }

    pub(crate) fn repo_module_compile_and_instantiate(
        surface_name: &'static str,
        module_path: impl Into<PathBuf>,
        export_name: Option<String>,
        language: QueryLanguage,
    ) -> Self {
        Self {
            surface_name,
            execution_product: PrismCodeExecutionProduct::CompileAndInstantiate,
            entry_point: PrismCodeEntryPoint::RepoModule(PrismCodeModuleEntryPoint {
                module_path: module_path.into(),
                export_name,
                language,
            }),
        }
    }

    pub(crate) fn surface_name(&self) -> &'static str {
        self.surface_name
    }

    pub(crate) fn execution_product(&self) -> PrismCodeExecutionProduct {
        self.execution_product
    }

    pub(crate) fn entry_point(&self) -> &PrismCodeEntryPoint {
        &self.entry_point
    }

    pub(crate) fn language(&self) -> QueryLanguage {
        match &self.entry_point {
            PrismCodeEntryPoint::InlineSnippet { language, .. } => language.clone(),
            PrismCodeEntryPoint::RepoModule(module) => module.language.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_input_classifies_read_and_write_products() {
        let read_only = PrismCodeCompilerInput::inline(
            "prism_code",
            "return prism.runtime.status();",
            QueryLanguage::Ts,
            false,
        );
        assert_eq!(
            read_only.execution_product(),
            PrismCodeExecutionProduct::ReadOnlyEvaluation
        );

        let write = PrismCodeCompilerInput::inline(
            "prism_code",
            "await prism.work.declare({ title: 'x' });",
            QueryLanguage::Ts,
            true,
        );
        assert_eq!(
            write.execution_product(),
            PrismCodeExecutionProduct::TransactionalWrite
        );
    }

    #[test]
    fn repo_module_inputs_classify_compilation_products() {
        let compile = PrismCodeCompilerInput::repo_module_compile(
            "prism_code",
            "plans/deploy.ts",
            Some("default".to_string()),
            QueryLanguage::Ts,
        );
        assert_eq!(
            compile.execution_product(),
            PrismCodeExecutionProduct::ReusableArtifactCompilation
        );

        let instantiate = PrismCodeCompilerInput::repo_module_compile_and_instantiate(
            "prism_code",
            "plans/deploy.ts",
            None,
            QueryLanguage::Ts,
        );
        assert_eq!(
            instantiate.execution_product(),
            PrismCodeExecutionProduct::CompileAndInstantiate
        );
    }
}
