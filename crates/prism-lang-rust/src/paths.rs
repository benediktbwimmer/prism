use prism_parser::{relative_package_file, ParseInput};
use tree_sitter::Node as TsNode;

use crate::syntax::node_text;

pub(crate) fn canonical_impl_parts(type_name: &str, trait_name: Option<&str>) -> (String, String) {
    if let Some(trait_name) = trait_name {
        (
            format!("{type_name}::impl::{trait_name}"),
            format!("{trait_name} for {type_name}"),
        )
    } else {
        (format!("{type_name}::impl"), type_name.to_owned())
    }
}

pub(crate) fn collect_use_paths(
    node: TsNode<'_>,
    prefix: Option<String>,
    source: &[u8],
) -> Vec<String> {
    match node.kind() {
        "use_declaration" => node
            .child_by_field_name("argument")
            .map(|argument| collect_use_paths(argument, prefix, source))
            .unwrap_or_default(),
        "scoped_use_list" => {
            let next_prefix = node
                .child_by_field_name("path")
                .map(|path| join_prefix(prefix.as_deref(), &node_text(path, source)));
            node.child_by_field_name("list")
                .map(|list| collect_use_paths(list, next_prefix, source))
                .unwrap_or_default()
        }
        "use_list" => {
            let mut paths = Vec::new();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                paths.extend(collect_use_paths(child, prefix.clone(), source));
            }
            paths
        }
        "use_as_clause" => node
            .child_by_field_name("path")
            .map(|path| collect_use_paths(path, prefix, source))
            .unwrap_or_default(),
        "use_wildcard" => Vec::new(),
        "crate" | "identifier" | "metavariable" | "scoped_identifier" | "self" | "super" => {
            vec![join_prefix(prefix.as_deref(), &node_text(node, source))]
        }
        _ => Vec::new(),
    }
}

fn join_prefix(prefix: Option<&str>, suffix: &str) -> String {
    match prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}::{suffix}"),
        _ => suffix.to_owned(),
    }
}

pub(crate) fn simplify_symbol(value: &str) -> String {
    let mut value = value.rsplit("::").next().unwrap_or(value).to_owned();
    if let Some((_, field)) = value.rsplit_once('.') {
        value = field.to_owned();
    }
    if let Some((head, _)) = value.split_once("::<") {
        value = head.to_owned();
    }
    if let Some(stripped) = value.strip_prefix("r#") {
        value = stripped.to_owned();
    }
    value
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .to_owned()
}

pub(crate) fn normalize_type_name(value: &str) -> String {
    value
        .replace("::", "_")
        .replace('<', "_")
        .replace('>', "")
        .replace(',', "_")
        .replace('&', "ref_")
        .replace('[', "_")
        .replace(']', "")
        .replace(' ', "")
}

pub(crate) fn canonical_symbol_path(value: &str, module_path: &str, crate_name: &str) -> String {
    let mut base = module_path.to_owned();
    let cleaned = value.replace(' ', "");
    let parts = cleaned
        .split("::")
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return base;
    }

    let mut index = 0usize;
    while index < parts.len() {
        match parts[index] {
            "crate" => {
                base = crate_name.to_owned();
                index += 1;
            }
            "self" => {
                index += 1;
            }
            "super" => {
                base = parent_module_path(&base).to_owned();
                index += 1;
            }
            _ => break,
        }
    }

    let remaining = parts[index..]
        .iter()
        .map(|segment| normalize_path_segment(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if remaining.is_empty() {
        return base;
    }

    format!("{base}::{}", remaining.join("::"))
}

fn normalize_path_segment(value: &str) -> String {
    simplify_symbol(
        &value
            .replace('<', "_")
            .replace('>', "")
            .replace(',', "_")
            .replace('&', "ref_")
            .replace('[', "_")
            .replace(']', ""),
    )
}

fn parent_module_path(value: &str) -> &str {
    value
        .rsplit_once("::")
        .map(|(parent, _)| parent)
        .unwrap_or(value)
}

pub(crate) fn module_path(input: &ParseInput<'_>) -> String {
    let relative = relative_package_file(input);
    let mut parts = vec![input.crate_name.to_owned()];
    let relative = relative.strip_prefix("src").unwrap_or(relative.as_path());
    let file_stem = relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();

    for component in relative
        .parent()
        .into_iter()
        .flat_map(|path| path.components())
    {
        parts.push(component.as_os_str().to_string_lossy().to_string());
    }

    if !matches!(file_stem, "lib" | "main" | "mod" | "") {
        parts.push(file_stem.to_owned());
    }

    parts.join("::")
}

pub(crate) fn last_segment(path: &str) -> Option<&str> {
    path.rsplit("::").next()
}
