use std::path::Path;

use anyhow::{Context, Result};
use prism_ir::{Language, Node, NodeId, NodeKind, Span, SymbolFingerprint};
use prism_parser::{
    fingerprint_from_parts, normalized_shape_hash, LanguageAdapter, ParseInput, ParseResult,
    UnresolvedCall, UnresolvedImpl, UnresolvedImport,
};
use smol_str::SmolStr;
use tree_sitter::{Node as TsNode, Parser};

use crate::paths::{
    canonical_impl_parts, canonical_symbol_path, collect_use_paths, last_segment, module_path,
    normalize_type_name,
};
use crate::syntax::{
    extract_calls, kind_label, node_name, node_span, node_text, push_contains_edge,
    push_fingerprinted_node,
};

pub struct RustAdapter;

fn fingerprint_from_signature_and_shape<I, S>(
    signature_parts: I,
    shape_hash: &str,
) -> SymbolFingerprint
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let signature = fingerprint_from_parts(signature_parts);
    let shape_hash = u64::from_str_radix(shape_hash, 16).unwrap_or(signature.signature_hash);
    SymbolFingerprint::with_parts(
        signature.signature_hash,
        Some(shape_hash),
        Some(shape_hash),
        None,
    )
}

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
        fingerprint_from_signature_and_shape(
            [
                "rust",
                kind_label(kind),
                param_count.to_string().as_str(),
                if node.child_by_field_name("return_type").is_some() {
                    "ret"
                } else {
                    "unit"
                },
            ],
            body_shape.as_str(),
        ),
    );
    push_contains_edge(result, scope.parent_id.clone(), id.clone());

    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    if !input.parse_depth.is_deep() {
        return;
    }
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
