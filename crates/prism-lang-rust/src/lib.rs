use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_parser::{
    fingerprint_from_parts, normalized_shape_hash, relative_package_file, LanguageAdapter,
    NodeFingerprint, ParseInput, ParseResult, UnresolvedCall, UnresolvedImpl, UnresolvedImport,
};
use smol_str::SmolStr;
use tree_sitter::{Node as TsNode, Parser};

pub struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs"))
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::LANGUAGE;
        parser
            .set_language(&language.into())
            .context("failed to load tree-sitter-rust grammar")?;

        let tree = parser
            .parse(input.source, None)
            .context("tree-sitter failed to parse Rust source")?;

        let module_path = module_path(input);
        let module_id = NodeId::new(input.crate_name, module_path.clone(), NodeKind::Module);
        let mut result = ParseResult::default();
        let module_node = Node {
            id: module_id.clone(),
            name: SmolStr::new(last_segment(&module_path).unwrap_or(input.crate_name)),
            kind: NodeKind::Module,
            file: input.file_id,
            span: Span::whole_file(input.source.len()),
            language: Language::Rust,
        };
        push_fingerprinted_node(
            &mut result,
            module_node,
            fingerprint_from_parts([
                "rust",
                "module",
                normalized_shape_hash(input.source).as_str(),
            ]),
        );

        let scope = Scope::module(module_id, module_path);
        walk_declarations(
            tree.root_node(),
            &scope,
            input,
            input.source.as_bytes(),
            &mut result,
        );
        Ok(result)
    }
}

#[derive(Clone)]
struct Scope {
    parent_id: NodeId,
    module_path: String,
    owner: ScopeOwner,
}

#[derive(Clone)]
enum ScopeOwner {
    Module,
    Struct(String),
    Trait(String),
    Impl(String),
}

impl Scope {
    fn module(parent_id: NodeId, module_path: String) -> Self {
        Self {
            parent_id,
            module_path,
            owner: ScopeOwner::Module,
        }
    }

    fn with_owner(&self, parent_id: NodeId, owner: ScopeOwner) -> Self {
        Self {
            parent_id,
            module_path: self.module_path.clone(),
            owner,
        }
    }

    fn nested_module(&self, parent_id: NodeId, module_name: &str) -> Self {
        Self {
            parent_id,
            module_path: format!("{}::{module_name}", self.module_path),
            owner: ScopeOwner::Module,
        }
    }

    fn function_path(&self, name: &str) -> String {
        match &self.owner {
            ScopeOwner::Module => format!("{}::{name}", self.module_path),
            ScopeOwner::Trait(target) | ScopeOwner::Impl(target) | ScopeOwner::Struct(target) => {
                format!("{}::{target}::{name}", self.module_path)
            }
        }
    }

    fn field_path(&self, name: &str) -> Option<String> {
        match &self.owner {
            ScopeOwner::Struct(target) => Some(format!("{}::{target}::{name}", self.module_path)),
            _ => None,
        }
    }
}

fn walk_declarations(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "source_file" | "declaration_list" => {
                walk_declarations(child, scope, input, source, result);
            }
            "mod_item" => parse_module(child, scope, input, source, result),
            "struct_item" => parse_struct(child, scope, input, source, result),
            "enum_item" => parse_named_item(child, scope, input, source, result, NodeKind::Enum),
            "trait_item" => parse_trait(child, scope, input, source, result),
            "impl_item" => parse_impl(child, scope, input, source, result),
            "function_item" => parse_function(child, scope, input, source, result),
            "use_declaration" => parse_use(child, scope, input, source, result),
            "type_item" => {
                parse_named_item(child, scope, input, source, result, NodeKind::TypeAlias)
            }
            "field_declaration" => parse_field(child, scope, input, source, result),
            _ => {}
        }
    }
}

fn parse_module(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(name) = node_name(node, source) else {
        return;
    };
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };

    let path = format!("{}::{name}", scope.module_path);
    let id = NodeId::new(input.crate_name, path, NodeKind::Module);
    let node = Node {
        id: id.clone(),
        name: SmolStr::new(name.clone()),
        kind: NodeKind::Module,
        file: input.file_id,
        span: node_span(node),
        language: Language::Rust,
    };
    push_fingerprinted_node(
        result,
        node,
        fingerprint_from_parts([
            "rust",
            "module",
            normalized_shape_hash(&node_text(body, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id.clone());

    let nested_scope = scope.nested_module(id, &name);
    walk_declarations(body, &nested_scope, input, source, result);
}

fn parse_struct(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(name) = node_name(node, source) else {
        return;
    };

    let path = format!("{}::{name}", scope.module_path);
    let id = NodeId::new(input.crate_name, path, NodeKind::Struct);
    let field_count = node
        .child_by_field_name("body")
        .map(|body| {
            let mut cursor = body.walk();
            body.named_children(&mut cursor)
                .filter(|child| child.kind() == "field_declaration")
                .count()
        })
        .unwrap_or(0);
    let struct_node = Node {
        id: id.clone(),
        name: SmolStr::new(name.clone()),
        kind: NodeKind::Struct,
        file: input.file_id,
        span: node_span(node),
        language: Language::Rust,
    };
    push_fingerprinted_node(
        result,
        struct_node,
        fingerprint_from_parts([
            "rust",
            "struct",
            field_count.to_string().as_str(),
            normalized_shape_hash(&node_text(node, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id.clone());

    if let Some(body) = node.child_by_field_name("body") {
        let field_scope = scope.with_owner(id, ScopeOwner::Struct(name));
        walk_declarations(body, &field_scope, input, source, result);
    }
}

fn parse_trait(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(name) = node_name(node, source) else {
        return;
    };

    let path = format!("{}::{name}", scope.module_path);
    let id = NodeId::new(input.crate_name, path, NodeKind::Trait);
    let method_count = node
        .child_by_field_name("body")
        .map(|body| {
            let mut cursor = body.walk();
            body.named_children(&mut cursor)
                .filter(|child| child.kind() == "function_item")
                .count()
        })
        .unwrap_or(0);
    let trait_node = Node {
        id: id.clone(),
        name: SmolStr::new(name.clone()),
        kind: NodeKind::Trait,
        file: input.file_id,
        span: node_span(node),
        language: Language::Rust,
    };
    push_fingerprinted_node(
        result,
        trait_node,
        fingerprint_from_parts([
            "rust",
            "trait",
            method_count.to_string().as_str(),
            normalized_shape_hash(&node_text(node, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id.clone());

    if let Some(body) = node.child_by_field_name("body") {
        let trait_scope = scope.with_owner(id, ScopeOwner::Trait(name));
        walk_declarations(body, &trait_scope, input, source, result);
    }
}

fn parse_impl(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(type_node) = node.child_by_field_name("type") else {
        return;
    };
    let type_name = normalize_type_name(&node_text(type_node, source));
    let trait_name = node
        .child_by_field_name("trait")
        .map(|trait_node| normalize_type_name(&node_text(trait_node, source)));
    let trait_path = node.child_by_field_name("trait").map(|trait_node| {
        canonical_symbol_path(
            &node_text(trait_node, source),
            &scope.module_path,
            input.crate_name,
        )
    });
    let (path_suffix, label) = canonical_impl_parts(&type_name, trait_name.as_deref());
    let id = NodeId::new(
        input.crate_name,
        format!("{}::{path_suffix}", scope.module_path),
        NodeKind::Impl,
    );
    let method_count = node
        .child_by_field_name("body")
        .map(|body| {
            let mut cursor = body.walk();
            body.named_children(&mut cursor)
                .filter(|child| child.kind() == "function_item")
                .count()
        })
        .unwrap_or(0);
    let impl_node = Node {
        id: id.clone(),
        name: SmolStr::new(label),
        kind: NodeKind::Impl,
        file: input.file_id,
        span: node_span(node),
        language: Language::Rust,
    };
    push_fingerprinted_node(
        result,
        impl_node,
        fingerprint_from_parts([
            "rust",
            "impl",
            if trait_path.is_some() {
                "trait"
            } else {
                "inherent"
            },
            method_count.to_string().as_str(),
            normalized_shape_hash(&node_text(node, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id.clone());

    if let Some(body) = node.child_by_field_name("body") {
        let impl_scope = scope.with_owner(id.clone(), ScopeOwner::Impl(type_name));
        walk_declarations(body, &impl_scope, input, source, result);
    }

    if let (Some(_trait_name), Some(trait_path)) = (trait_name, trait_path) {
        result.unresolved_impls.push(UnresolvedImpl {
            impl_node: id,
            target: SmolStr::new(trait_path),
            span: node_span(node),
            module_path: SmolStr::new(scope.module_path.clone()),
        });
    }
}

fn parse_named_item(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
    kind: NodeKind,
) {
    let Some(name) = node_name(node, source) else {
        return;
    };

    let path = format!("{}::{name}", scope.module_path);
    let id = NodeId::new(input.crate_name, path, kind);
    let item_node = Node {
        id: id.clone(),
        name: SmolStr::new(name),
        kind,
        file: input.file_id,
        span: node_span(node),
        language: Language::Rust,
    };
    push_fingerprinted_node(
        result,
        item_node,
        fingerprint_from_parts([
            "rust",
            kind_label(kind),
            normalized_shape_hash(&node_text(node, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id);
}

fn parse_field(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(name) = node_name(node, source) else {
        return;
    };
    let Some(path) = scope.field_path(&name) else {
        return;
    };

    let id = NodeId::new(input.crate_name, path, NodeKind::Field);
    let field_node = Node {
        id: id.clone(),
        name: SmolStr::new(name),
        kind: NodeKind::Field,
        file: input.file_id,
        span: node_span(node),
        language: Language::Rust,
    };
    push_fingerprinted_node(
        result,
        field_node,
        fingerprint_from_parts([
            "rust",
            "field",
            normalized_shape_hash(&node_text(node, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id);
}

fn parse_function(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(name) = node_name(node, source) else {
        return;
    };

    let kind = match scope.owner {
        ScopeOwner::Module => NodeKind::Function,
        _ => NodeKind::Method,
    };
    let id = NodeId::new(input.crate_name, scope.function_path(&name), kind);
    let param_count = node
        .child_by_field_name("parameters")
        .map(|parameters| {
            let mut cursor = parameters.walk();
            parameters
                .named_children(&mut cursor)
                .filter(|child| matches!(child.kind(), "parameter" | "self_parameter"))
                .count()
        })
        .unwrap_or(0);
    let body_shape = node
        .child_by_field_name("body")
        .map(|body| normalized_shape_hash(&node_text(body, source)))
        .unwrap_or_else(|| normalized_shape_hash(""));
    let function_node = Node {
        id: id.clone(),
        name: SmolStr::new(name),
        kind,
        file: input.file_id,
        span: node_span(node),
        language: Language::Rust,
    };
    push_fingerprinted_node(
        result,
        function_node,
        fingerprint_from_parts([
            "rust",
            kind_label(kind),
            param_count.to_string().as_str(),
            if node.child_by_field_name("return_type").is_some() {
                "ret"
            } else {
                "unit"
            },
            body_shape.as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id.clone());

    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    for (call, span) in extract_calls(body, source) {
        result.unresolved_calls.push(UnresolvedCall {
            caller: id.clone(),
            name: SmolStr::new(call),
            span,
            module_path: SmolStr::new(scope.module_path.clone()),
        });
    }
}

fn parse_use(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(argument) = node.child_by_field_name("argument") else {
        return;
    };

    for raw_path in collect_use_paths(argument, None, source) {
        let canonical = canonical_symbol_path(&raw_path, &scope.module_path, input.crate_name);
        result.unresolved_imports.push(UnresolvedImport {
            importer: scope.parent_id.clone(),
            path: SmolStr::new(canonical),
            span: node_span(argument),
            module_path: SmolStr::new(scope.module_path.clone()),
        });
    }
}

fn extract_calls(node: TsNode<'_>, source: &[u8]) -> Vec<(String, Span)> {
    let mut calls = BTreeMap::new();
    collect_calls(node, source, &mut calls);
    calls.into_iter().collect()
}

fn collect_calls(node: TsNode<'_>, source: &[u8], calls: &mut BTreeMap<String, Span>) {
    if node.kind() == "call_expression" {
        if let Some(name) = extract_call_name(node, source) {
            calls.entry(name).or_insert_with(|| node_span(node));
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_calls(child, source, calls);
    }
}

fn extract_call_name(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    let function = node.child_by_field_name("function")?;
    extract_called_symbol(function, source)
}

fn extract_called_symbol(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" | "self" => {
            Some(simplify_symbol(&node_text(node, source)))
        }
        "scoped_identifier" => Some(simplify_symbol(&node_text(node, source))),
        "field_expression" => node
            .child_by_field_name("field")
            .map(|field| simplify_symbol(&node_text(field, source))),
        "generic_function" => node
            .child_by_field_name("function")
            .and_then(|function| extract_called_symbol(function, source)),
        _ => None,
    }
}

fn node_name(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    Some(node_text(node.child_by_field_name("name")?, source))
}

fn node_text(node: TsNode<'_>, source: &[u8]) -> String {
    node.utf8_text(source).unwrap_or_default().to_owned()
}

fn node_span(node: TsNode<'_>) -> Span {
    Span::new(node.start_byte(), node.end_byte())
}

fn push_contains_edge(result: &mut ParseResult, source: NodeId, target: NodeId) {
    result.edges.push(Edge {
        kind: EdgeKind::Contains,
        source,
        target,
        origin: EdgeOrigin::Static,
        confidence: 1.0,
    });
}

fn push_fingerprinted_node(result: &mut ParseResult, node: Node, fingerprint: NodeFingerprint) {
    result.record_fingerprint(&node.id, fingerprint);
    result.nodes.push(node);
}

fn kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Module => "module",
        NodeKind::Function => "function",
        NodeKind::Struct => "struct",
        NodeKind::Enum => "enum",
        NodeKind::Trait => "trait",
        NodeKind::Impl => "impl",
        NodeKind::Method => "method",
        NodeKind::Field => "field",
        NodeKind::TypeAlias => "type_alias",
        _ => "node",
    }
}

fn canonical_impl_parts(type_name: &str, trait_name: Option<&str>) -> (String, String) {
    if let Some(trait_name) = trait_name {
        (
            format!("{type_name}::impl::{trait_name}"),
            format!("{trait_name} for {type_name}"),
        )
    } else {
        (format!("{type_name}::impl"), type_name.to_owned())
    }
}

fn collect_use_paths(node: TsNode<'_>, prefix: Option<String>, source: &[u8]) -> Vec<String> {
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

fn simplify_symbol(value: &str) -> String {
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

fn normalize_type_name(value: &str) -> String {
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

fn canonical_symbol_path(value: &str, module_path: &str, crate_name: &str) -> String {
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

    if index == 0 {
        format!("{base}::{}", remaining.join("::"))
    } else {
        format!("{base}::{}", remaining.join("::"))
    }
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

fn module_path(input: &ParseInput<'_>) -> String {
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

fn last_segment(path: &str) -> Option<&str> {
    path.rsplit("::").next()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::{FileId, NodeKind};
    use prism_parser::{LanguageAdapter, ParseInput};

    use super::RustAdapter;

    #[test]
    fn parses_top_level_function_and_call() {
        let adapter = RustAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/src/lib.rs"),
            file_id: FileId(1),
            source: "fn alpha() { beta(); }\nfn beta() {}\n",
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Function && node.id.path == "demo::alpha"));
        assert!(result
            .unresolved_calls
            .iter()
            .any(|call| call.caller.path == "demo::alpha" && call.name == "beta"));
        assert!(!result
            .unresolved_calls
            .iter()
            .any(|call| call.name == "alpha"));
    }

    #[test]
    fn parses_impls_nested_modules_and_fields() {
        let adapter = RustAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/src/lib.rs"),
            file_id: FileId(1),
            source: r#"
struct Config {
    value: usize,
}

trait Service {
    fn run(&self) -> usize;
}

impl Service for Config {
    fn run(&self) -> usize {
        helper()
    }
}

fn helper() -> usize { 1 }

mod nested {
    fn ping() {}
}
"#,
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Field && node.id.path == "demo::Config::value"));
        assert!(result.nodes.iter().any(
            |node| node.kind == NodeKind::Impl && node.id.path == "demo::Config::impl::Service"
        ));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Method && node.id.path == "demo::Config::run"));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Module && node.id.path == "demo::nested"));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Function && node.id.path == "demo::nested::ping"));
        assert!(result
            .unresolved_calls
            .iter()
            .any(|call| call.caller.path == "demo::Config::run" && call.name == "helper"));
    }

    #[test]
    fn collects_imports_and_trait_references() {
        let adapter = RustAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("/workspace"),
            path: Path::new("/workspace/src/lib.rs"),
            file_id: FileId(1),
            source: r#"
use crate::net::Client;
use self::models::{User as AppUser, Account};
use super::shared::Thing;

trait Runner {}
struct Job;

impl Runner for Job {}
"#,
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .unresolved_imports
            .iter()
            .any(|import| import.path == "demo::net::Client"));
        assert!(result
            .unresolved_imports
            .iter()
            .any(|import| import.path == "demo::models::User"));
        assert!(result
            .unresolved_imports
            .iter()
            .any(|import| import.path == "demo::models::Account"));
        assert!(result
            .unresolved_impls
            .iter()
            .any(|implementation| implementation.target == "demo::Runner"));
    }
}
