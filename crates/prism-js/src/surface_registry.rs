use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::compiler_surface::prism_compiler_method_specs;
use crate::query_surface::prism_api_method_specs;

pub fn runtime_option_keys_js_object() -> &'static str {
    static JS: OnceLock<String> = OnceLock::new();
    JS.get_or_init(|| {
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for bundle in prism_api_method_specs()
            .iter()
            .filter_map(|spec| spec.record_arg)
            .chain(
                prism_compiler_method_specs()
                    .iter()
                    .filter_map(|spec| spec.api.record_arg),
            )
        {
            if !seen.insert(bundle.bundle_name) {
                continue;
            }
            let keys = bundle
                .allowed_keys
                .iter()
                .map(|key| format!("\"{key}\""))
                .collect::<Vec<_>>()
                .join(", ");
            entries.push(format!(
                "  {}: Object.freeze([{}])",
                bundle.bundle_name, keys
            ));
        }
        format!("Object.freeze({{\n{}\n}})", entries.join(",\n"))
    })
    .as_str()
}
