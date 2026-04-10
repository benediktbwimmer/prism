use std::path::Path;

use anyhow::Result;

use super::source::{PrismCodeSourceBundle, PrismCodeSourceUnit};
use super::{load_compiler_sources, PrismCodeCompilerInput};
use crate::{
    QUERY_RUNTIME_ERROR_MARKER, QUERY_SERIALIZATION_ERROR_MARKER, USER_SNIPPET_LOCATION_MARKER,
    USER_SNIPPET_MARKER,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrismTypescriptProgramMode {
    StatementBody,
    ImplicitExpression,
}

impl PrismTypescriptProgramMode {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::StatementBody => "statement_body",
            Self::ImplicitExpression => "implicit_expression",
        }
    }

    fn wrap_user_body(self, code: &str) -> String {
        match self {
            Self::StatementBody => code.to_string(),
            Self::ImplicitExpression => format!("return (\n{}\n);", code),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedTypescriptProgram {
    source_bundle: PrismCodeSourceBundle,
    wrapped_source: String,
    user_snippet_first_line: usize,
    mode: PrismTypescriptProgramMode,
}

impl PreparedTypescriptProgram {
    pub(crate) fn root_source(&self) -> &PrismCodeSourceUnit {
        self.source_bundle.root()
    }

    pub(crate) fn wrapped_source(&self) -> &str {
        &self.wrapped_source
    }

    pub(crate) fn user_snippet_first_line(&self) -> usize {
        self.user_snippet_first_line
    }

    pub(crate) fn mode(&self) -> PrismTypescriptProgramMode {
        self.mode
    }
}

pub(crate) fn prepare_typescript_program(
    input: &PrismCodeCompilerInput,
    workspace_root: Option<&Path>,
    mode: PrismTypescriptProgramMode,
) -> Result<PreparedTypescriptProgram> {
    let source_bundle = load_compiler_sources(input, workspace_root)?;
    let user_body = mode.wrap_user_body(source_bundle.root().source_text());
    let wrapped_source = format!(
        "(async function() {{\n  const __prismLocationRegex = /(?:file:\\/\\/\\/prism\\/query\\.ts|eval_script):(?<line>\\d+):(?<column>\\d+)/;\n  const __prismParseLocation = (value) => {{\n    const __prismMatch = typeof value === \"string\" ? value.match(__prismLocationRegex) : null;\n    if (!__prismMatch || !__prismMatch.groups) {{\n      return null;\n    }};\n    return {{\n      line: Number(__prismMatch.groups.line),\n      column: Number(__prismMatch.groups.column),\n    }};\n  }};\n  const __prismFormatError = (error) => {{\n    const __prismMessage = error && typeof error === \"object\" && \"message\" in error && error.message\n      ? String(error.message)\n      : String(error);\n    const __prismStack = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : null;\n    return __prismStack && __prismStack.includes(__prismMessage)\n      ? __prismStack\n      : __prismStack\n        ? `${{__prismMessage}}\\n${{__prismStack}}`\n        : __prismMessage;\n  }};\n  const __prismUserLocation = (error, baseLine) => {{\n    if (typeof baseLine !== \"number\") {{\n      return null;\n    }}\n    const __prismStack = error && typeof error === \"object\" && \"stack\" in error && error.stack\n      ? String(error.stack)\n      : \"\";\n    const __prismLines = __prismStack.split(\"\\n\");\n    const __prismFrame = __prismLines.find((line) => line.includes(\"__prismUserQuery\"))\n      || __prismLines.find((line) => line.includes(\"eval_script:\"));\n    const __prismLocation = __prismParseLocation(__prismFrame);\n    if (!__prismLocation) {{\n      return null;\n    }}\n    return {{\n      line: Math.max(1, __prismLocation.line - baseLine + 1),\n      column: __prismLocation.column,\n    }};\n  }};\n  const __prismThrowTaggedError = (marker, error, userLocation = null) => {{\n    const __prismFormatted = __prismFormatError(error);\n    const __prismHeadline = __prismFormatted.split(\"\\n\")[0] || String(error);\n    const __prismUserLocationLine = userLocation\n      ? `\\n{} ${{userLocation.line}}:${{userLocation.column}}`\n      : \"\";\n    const __prismWrapped = new Error(`${{marker}}\\n${{__prismHeadline}}${{__prismUserLocationLine}}`);\n    __prismWrapped.stack = `${{userLocation ? `{} ${{userLocation.line}}:${{userLocation.column}}\\n` : \"\"}}${{__prismFormatted}}`;\n    throw __prismWrapped;\n  }};\n  let __prismUserSnippetBaseLine = null;\n  const __prismUserQuery = async () => {{\n    const __prismBaseLocation = __prismParseLocation(new Error().stack || \"\");\n    __prismUserSnippetBaseLine = __prismBaseLocation ? __prismBaseLocation.line + 1 : null;\n{}\n{}\n  }};\n  let __prismResult;\n  try {{\n    __prismResult = await __prismUserQuery();\n  }} catch (error) {{\n    __prismThrowTaggedError(\"{}\", error, __prismUserLocation(error, __prismUserSnippetBaseLine));\n  }}\n  try {{\n    __prismResult = __prismHost(\"__finalizeCode\", {{ result: __prismResult }});\n    return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n  }} catch (error) {{\n    __prismThrowTaggedError(\"{}\", error);\n  }}\n}})();\n",
        USER_SNIPPET_LOCATION_MARKER,
        USER_SNIPPET_LOCATION_MARKER,
        USER_SNIPPET_MARKER,
        user_body,
        QUERY_RUNTIME_ERROR_MARKER,
        QUERY_SERIALIZATION_ERROR_MARKER,
    );
    let user_snippet_first_line = wrapped_source
        .lines()
        .position(|line| line.trim() == USER_SNIPPET_MARKER)
        .map(|index| index + 2)
        .unwrap_or(1);

    Ok(PreparedTypescriptProgram {
        source_bundle,
        wrapped_source,
        user_snippet_first_line,
        mode,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::QueryLanguage;

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "prism_code_compiler_entry_tests_{}_{}",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn prepared_program_wraps_inline_source() {
        let input = PrismCodeCompilerInput::inline(
            "prism_code",
            "return prism.runtimeStatus();",
            QueryLanguage::Ts,
            false,
        );
        let prepared =
            prepare_typescript_program(&input, None, PrismTypescriptProgramMode::StatementBody)
                .expect("inline program should prepare");
        assert!(prepared.wrapped_source().contains(USER_SNIPPET_MARKER));
        assert!(prepared
            .wrapped_source()
            .contains("return prism.runtimeStatus();"));
        assert!(prepared.user_snippet_first_line() > 0);
    }

    #[test]
    fn prepared_program_uses_repo_module_specifier() {
        let root = unique_temp_dir();
        let module_dir = root.join(".prism/code/plans");
        fs::create_dir_all(&module_dir).expect("module directory should create");
        fs::write(
            module_dir.join("deploy.ts"),
            "export default async function deploy() { return 1; }\n",
        )
        .expect("module should write");

        let input = PrismCodeCompilerInput::repo_module_compile(
            "prism_code",
            "plans/deploy.ts",
            Some("default".to_string()),
            QueryLanguage::Ts,
        );
        let prepared = prepare_typescript_program(
            &input,
            Some(&root),
            PrismTypescriptProgramMode::StatementBody,
        )
        .expect("repo module program should prepare");
        assert_eq!(
            prepared.root_source().specifier(),
            "file:///prism/code/plans/deploy.ts"
        );
        assert!(prepared
            .wrapped_source()
            .contains("export default async function deploy()"));

        fs::remove_dir_all(&root).expect("temporary test directory should remove");
    }
}
