use std::sync::OnceLock;

use crate::query_surface::{PrismApiMethodSpec, PrismCompilerEffectKind, prism_api_method_specs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrismCompilerMethodSpec {
    pub api: PrismApiMethodSpec,
    pub effect: PrismCompilerEffectKind,
    pub host_operation: Option<&'static str>,
}

pub fn prism_compiler_method_specs() -> &'static [PrismCompilerMethodSpec] {
    static SPECS: OnceLock<Vec<PrismCompilerMethodSpec>> = OnceLock::new();
    SPECS
        .get_or_init(|| {
            prism_api_method_specs()
                .iter()
                .filter_map(|spec| {
                    spec.compiler.map(|compiler| PrismCompilerMethodSpec {
                        api: *spec,
                        effect: compiler.effect,
                        host_operation: compiler.host_operation,
                    })
                })
                .collect()
        })
        .as_slice()
}

pub fn prism_compiler_method_spec(path: &str) -> Option<&'static PrismCompilerMethodSpec> {
    prism_compiler_method_specs()
        .iter()
        .find(|spec| spec.api.path == path)
}

pub fn prism_compiler_method_spec_by_host_operation(
    host_operation: &str,
) -> Option<&'static PrismCompilerMethodSpec> {
    prism_compiler_method_specs()
        .iter()
        .find(|spec| spec.host_operation == Some(host_operation))
}
