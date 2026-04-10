use anyhow::Result;

use super::{PreparedTypescriptProgram, PrismTypescriptProgramMode};
use crate::js_runtime;
use crate::query_errors::parse_typescript_error;
use crate::query_typecheck::{typecheck_query_with_specifier, StaticCheckMode};

fn static_check_mode(mode: PrismTypescriptProgramMode) -> StaticCheckMode {
    match mode {
        PrismTypescriptProgramMode::StatementBody => StaticCheckMode::StatementBody,
        PrismTypescriptProgramMode::ImplicitExpression => StaticCheckMode::ImplicitExpression,
    }
}

pub(crate) fn typecheck_prepared_typescript_program(
    prepared: &PreparedTypescriptProgram,
) -> Result<()> {
    typecheck_query_with_specifier(
        prepared.root_source().source_text(),
        static_check_mode(prepared.mode()),
        prepared.root_source().specifier(),
    )
}

pub(crate) fn transpile_prepared_typescript_program(
    prepared: &PreparedTypescriptProgram,
) -> Result<String> {
    js_runtime::transpile_typescript_with_specifier(
        prepared.wrapped_source(),
        prepared.root_source().specifier(),
    )
    .map_err(|error| {
        parse_typescript_error(
            error,
            prepared.root_source().source_text(),
            prepared.user_snippet_first_line(),
            prepared.mode().code(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prism_code_compiler::{prepare_typescript_program, PrismCodeCompilerInput};
    use crate::QueryLanguage;

    #[test]
    fn typecheck_and_transpile_prepared_program() {
        let input = PrismCodeCompilerInput::inline(
            "prism_code",
            "return prism.runtimeStatus();",
            QueryLanguage::Ts,
            false,
        );
        let prepared =
            prepare_typescript_program(&input, None, PrismTypescriptProgramMode::StatementBody)
                .expect("program should prepare");

        typecheck_prepared_typescript_program(&prepared)
            .expect("prepared program should typecheck");
        let transpiled = transpile_prepared_typescript_program(&prepared)
            .expect("prepared program should transpile");
        assert!(transpiled.contains("__prismUserQuery"));
    }
}
