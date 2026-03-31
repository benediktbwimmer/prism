use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use prism_ir::{EdgeKind, Language, Node, NodeId, NodeKind, Span, SymbolFingerprint};
use prism_parser::{
    fingerprint_from_parts, normalized_shape_hash, LanguageAdapter, ParseInput, ParseResult,
    UnresolvedCall, UnresolvedImport, UnresolvedIntent,
};
use smol_str::SmolStr;
use tree_sitter::{Node as TsNode, Parser};

use crate::paths::{
    absolute_symbol_path, dotted_reference_target, import_path, is_package_init, module_path,
    simplify_symbol, split_relative_module_spec,
};
use crate::syntax::{
    extract_calls, extract_class_field_names, kind_label, node_name, node_span, node_text,
    push_contains_edge, push_fingerprinted_node,
};

pub struct PythonAdapter;

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

impl LanguageAdapter for PythonAdapter {
    fn language(&self) -> Language {
        Language::Python
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("py"))
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let mut parser = Parser::new();
        let language = tree_sitter_python::LANGUAGE;
        parser
            .set_language(&language.into())
            .context("failed to load tree-sitter-python grammar")?;

        let tree = parser
            .parse(input.source, None)
            .context("tree-sitter failed to parse Python source")?;

        let module_path = module_path(input);
        let module_id = NodeId::new(input.crate_name, module_path.clone(), NodeKind::Module);
        let mut result = ParseResult::default();
        let module_node = Node {
            id: module_id.clone(),
            name: SmolStr::new(last_segment(&module_path).unwrap_or(input.crate_name)),
            kind: NodeKind::Module,
            file: input.file_id,
            span: Span::whole_file(input.source.len()),
            language: Language::Python,
        };
        push_fingerprinted_node(
            &mut result,
            module_node,
            fingerprint_from_parts([
                "python",
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

    fn function_path(&self, name: &str) -> String {
        match &self.owner {
            ScopeOwner::Module => format!("{}::{name}", self.module_path),
            ScopeOwner::Struct(target) => format!("{}::{target}::{name}", self.module_path),
        }
    }

    fn field_path(&self, name: &str) -> Option<String> {
        match &self.owner {
            ScopeOwner::Struct(target) => Some(format!("{}::{target}::{name}", self.module_path)),
            ScopeOwner::Module => None,
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
            "module" | "block" | "expression_statement" => {
                walk_declarations(child, scope, input, source, result)
            }
            "class_definition" => parse_class(child, scope, input, source, result),
            "function_definition" => parse_function(child, scope, input, source, result),
            "decorated_definition" => {
                parse_decorated_definition(child, scope, input, source, result)
            }
            "import_statement" => parse_import(child, scope, input, source, result),
            "import_from_statement" => parse_import_from(child, scope, input, source, result),
            "future_import_statement" => parse_future_import(child, scope, input, source, result),
            "type_alias_statement" => parse_type_alias(child, scope, input, source, result),
            "assignment" | "augmented_assignment" => {
                parse_class_assignment_fields(child, scope, input, source, result)
            }
            _ => {}
        }
    }
}

fn parse_decorated_definition(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "class_definition" => parse_class(child, scope, input, source, result),
            "function_definition" => parse_function(child, scope, input, source, result),
            "type_alias_statement" => parse_type_alias(child, scope, input, source, result),
            _ => {}
        }
    }
}

fn parse_class(
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
    let member_count = node
        .child_by_field_name("body")
        .map(|body| {
            let mut cursor = body.walk();
            body.named_children(&mut cursor)
                .filter(|child| {
                    matches!(
                        child.kind(),
                        "class_definition"
                            | "function_definition"
                            | "assignment"
                            | "augmented_assignment"
                    )
                })
                .count()
        })
        .unwrap_or(0);
    let class_node = Node {
        id: id.clone(),
        name: SmolStr::new(name.clone()),
        kind: NodeKind::Struct,
        file: input.file_id,
        span: node_span(node),
        language: Language::Python,
    };
    push_fingerprinted_node(
        result,
        class_node,
        fingerprint_from_parts([
            "python",
            "class",
            member_count.to_string().as_str(),
            normalized_shape_hash(&node_text(node, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id.clone());

    if let Some(superclasses) = node.child_by_field_name("superclasses") {
        for target in extract_superclass_targets(superclasses, source, input.crate_name) {
            result.unresolved_intents.push(UnresolvedIntent {
                source: id.clone(),
                kind: EdgeKind::RelatedTo,
                target: SmolStr::new(target),
                span: node_span(superclasses),
            });
        }
    }

    if let Some(body) = node.child_by_field_name("body") {
        let class_scope = scope.with_owner(id, ScopeOwner::Struct(name));
        walk_declarations(body, &class_scope, input, source, result);
    }
}

fn parse_type_alias(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    let name = simplify_symbol(&node_text(left, source));
    if name.is_empty() {
        return;
    }

    let id = NodeId::new(
        input.crate_name,
        format!("{}::{name}", scope.module_path),
        NodeKind::TypeAlias,
    );
    let alias_node = Node {
        id: id.clone(),
        name: SmolStr::new(name),
        kind: NodeKind::TypeAlias,
        file: input.file_id,
        span: node_span(node),
        language: Language::Python,
    };
    push_fingerprinted_node(
        result,
        alias_node,
        fingerprint_from_parts([
            "python",
            "type_alias",
            normalized_shape_hash(&node_text(node, source)).as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id);
}

fn parse_class_assignment_fields(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    for (field_name, span) in extract_class_field_names(node, source) {
        push_field_node_if_missing(&field_name, span, scope, input, source, result);
    }
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
        ScopeOwner::Struct(_) => NodeKind::Method,
    };
    let id = NodeId::new(input.crate_name, scope.function_path(&name), kind);
    let param_count = node
        .child_by_field_name("parameters")
        .map(|parameters| {
            let mut cursor = parameters.walk();
            parameters.named_children(&mut cursor).count()
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
        language: Language::Python,
    };
    push_fingerprinted_node(
        result,
        function_node,
        fingerprint_from_signature_and_shape(
            [
                "python",
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

fn parse_import(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let raw_path = match child.kind() {
            "dotted_name" => node_text(child, source),
            "aliased_import" => child
                .child_by_field_name("name")
                .map(|name| node_text(name, source))
                .unwrap_or_default(),
            _ => continue,
        };
        if raw_path.is_empty() {
            continue;
        }
        let canonical = absolute_symbol_path(&raw_path, input.crate_name);
        result.unresolved_imports.push(UnresolvedImport {
            importer: scope.parent_id.clone(),
            path: SmolStr::new(canonical),
            span: node_span(child),
            module_path: SmolStr::new(scope.module_path.clone()),
        });
    }
}

fn parse_import_from(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(module_name) = node.child_by_field_name("module_name") else {
        return;
    };
    let module_spec = node_text(module_name, source);
    let (level, module_text) = split_relative_module_spec(&module_spec);
    let current_is_package = is_package_init(input);
    let module_range = (module_name.start_byte(), module_name.end_byte());
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        if (child.start_byte(), child.end_byte()) == module_range {
            continue;
        }
        let imported_name = match child.kind() {
            "wildcard_import" => None,
            "dotted_name" => Some(node_text(child, source)),
            "aliased_import" => child
                .child_by_field_name("name")
                .map(|name| node_text(name, source)),
            _ => continue,
        };
        let canonical = import_path(
            module_text,
            imported_name.as_deref(),
            level,
            &scope.module_path,
            input.crate_name,
            current_is_package,
        );
        result.unresolved_imports.push(UnresolvedImport {
            importer: scope.parent_id.clone(),
            path: SmolStr::new(canonical),
            span: node_span(child),
            module_path: SmolStr::new(scope.module_path.clone()),
        });
    }
}

fn parse_future_import(
    node: TsNode<'_>,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let raw_path = match child.kind() {
            "dotted_name" => Some(node_text(child, source)),
            "aliased_import" => child
                .child_by_field_name("name")
                .map(|name| node_text(name, source)),
            _ => None,
        };
        let Some(raw_path) = raw_path else {
            continue;
        };
        let canonical = import_path(
            Some("__future__"),
            Some(raw_path.as_str()),
            0,
            &scope.module_path,
            input.crate_name,
            is_package_init(input),
        );
        result.unresolved_imports.push(UnresolvedImport {
            importer: scope.parent_id.clone(),
            path: SmolStr::new(canonical),
            span: node_span(child),
            module_path: SmolStr::new(scope.module_path.clone()),
        });
    }
}

fn extract_superclass_targets(node: TsNode<'_>, source: &[u8], crate_name: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = HashSet::<String>::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let Some(target) = extract_superclass_target(child, source, crate_name) else {
            continue;
        };
        if seen.insert(target.clone()) {
            targets.push(target);
        }
    }
    targets
}

fn extract_superclass_target(node: TsNode<'_>, source: &[u8], crate_name: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "dotted_name" | "attribute" => Some(dotted_reference_target(
            &node_text(node, source),
            crate_name,
        )),
        _ => None,
    }
}

fn push_field_node_if_missing(
    field_name: &str,
    span: Span,
    scope: &Scope,
    input: &ParseInput<'_>,
    source: &[u8],
    result: &mut ParseResult,
) {
    let Some(path) = scope.field_path(field_name) else {
        return;
    };
    let id = NodeId::new(input.crate_name, path, NodeKind::Field);
    if result.nodes.iter().any(|node| node.id == id) {
        return;
    }

    let field_node = Node {
        id: id.clone(),
        name: SmolStr::new(field_name),
        kind: NodeKind::Field,
        file: input.file_id,
        span,
        language: Language::Python,
    };
    push_fingerprinted_node(
        result,
        field_node,
        fingerprint_from_parts([
            "python",
            "field",
            normalized_shape_hash(
                std::str::from_utf8(&source[span.start as usize..span.end as usize])
                    .unwrap_or_default(),
            )
            .as_str(),
        ]),
    );
    push_contains_edge(result, scope.parent_id.clone(), id);
}

fn last_segment(path: &str) -> Option<&str> {
    path.rsplit("::").next()
}
