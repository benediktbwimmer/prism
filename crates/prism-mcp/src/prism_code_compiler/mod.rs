mod analysis;
mod entry;
mod input;
mod program_ir;
mod source;
mod typescript;

pub(crate) use analysis::analyze_prepared_typescript_program;
pub(crate) use entry::{
    prepare_typescript_program, PreparedTypescriptProgram, PrismTypescriptProgramMode,
};
pub(crate) use input::PrismCodeCompilerInput;
pub(crate) use source::load_compiler_sources;
pub(crate) use typescript::{
    transpile_prepared_typescript_program, typecheck_prepared_typescript_program,
};
