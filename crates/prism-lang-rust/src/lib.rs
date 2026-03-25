use std::path::Path;

use anyhow::Result;
use prism_ir::{Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span};
use prism_parser::{relative_file, LanguageAdapter, ParseInput, ParseResult, UnresolvedCall};
use regex::Regex;
use smol_str::SmolStr;

pub struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn supports_path(&self, path: &Path) -> bool {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("rs"))
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let module_path = module_path(input);
        let mut result = ParseResult::default();

        let module_id = NodeId::new(input.crate_name, module_path.clone(), NodeKind::Module);
        result.nodes.push(Node {
            id: module_id.clone(),
            name: SmolStr::new(last_segment(&module_path).unwrap_or(input.crate_name)),
            kind: NodeKind::Module,
            file: input.file_id,
            span: Span::whole_file(input.source.lines().count()),
            language: Language::Rust,
        });

        let mut container_stack: Vec<Container> = Vec::new();
        let mut function_capture: Option<FunctionCapture> = None;

        for (line_index, raw_line) in input.source.lines().enumerate() {
            let line_number = line_index + 1;
            let line = strip_comment(raw_line);
            let trimmed = line.trim();
            let net_braces = count_braces(&line);

            if let Some(function) = function_capture.as_mut() {
                function.body.push_str(raw_line);
                function.body.push('\n');
                function.brace_balance += net_braces;
                if function.brace_balance <= 0 {
                    finalize_function(&mut result, function.clone());
                    function_capture = None;
                }
                update_containers(&mut container_stack, net_braces);
                continue;
            }

            if trimmed.is_empty() {
                update_containers(&mut container_stack, net_braces);
                continue;
            }

            if let Some((id, name)) = parse_struct(input, &module_path, line_number, trimmed) {
                push_node(&mut result, id, name, NodeKind::Struct, input, line_number);
                add_contains_edge(&mut result, &module_id, &container_stack, NodeKind::Struct);
                update_containers(&mut container_stack, net_braces);
                continue;
            }

            if let Some((id, name)) = parse_enum(input, &module_path, line_number, trimmed) {
                push_node(&mut result, id, name, NodeKind::Enum, input, line_number);
                add_contains_edge(&mut result, &module_id, &container_stack, NodeKind::Enum);
                update_containers(&mut container_stack, net_braces);
                continue;
            }

            if let Some((id, name)) = parse_type_alias(input, &module_path, line_number, trimmed) {
                push_node(
                    &mut result,
                    id,
                    name,
                    NodeKind::TypeAlias,
                    input,
                    line_number,
                );
                add_contains_edge(
                    &mut result,
                    &module_id,
                    &container_stack,
                    NodeKind::TypeAlias,
                );
                update_containers(&mut container_stack, net_braces);
                continue;
            }

            if let Some((id, name)) = parse_trait(input, &module_path, line_number, trimmed) {
                push_node(
                    &mut result,
                    id.clone(),
                    name.clone(),
                    NodeKind::Trait,
                    input,
                    line_number,
                );
                result.edges.push(Edge {
                    kind: EdgeKind::Contains,
                    source: module_id.clone(),
                    target: id.clone(),
                    origin: EdgeOrigin::Static,
                    confidence: 1.0,
                });
                container_stack.push(Container::new(ContainerKind::Trait(name), id, net_braces));
                pop_closed_containers(&mut container_stack);
                continue;
            }

            if let Some((id, label, target_name)) =
                parse_impl(input, &module_path, line_number, trimmed)
            {
                push_node(
                    &mut result,
                    id.clone(),
                    label.clone(),
                    NodeKind::Impl,
                    input,
                    line_number,
                );
                result.edges.push(Edge {
                    kind: EdgeKind::Contains,
                    source: module_id.clone(),
                    target: id.clone(),
                    origin: EdgeOrigin::Static,
                    confidence: 1.0,
                });
                container_stack.push(Container::new(
                    ContainerKind::Impl(target_name),
                    id,
                    net_braces,
                ));
                pop_closed_containers(&mut container_stack);
                continue;
            }

            if let Some(function) = parse_function(
                input,
                &module_path,
                line_number,
                trimmed,
                container_stack.last(),
            ) {
                let kind = function.kind;
                let parent_id = function.parent.clone().unwrap_or_else(|| module_id.clone());
                result.nodes.push(Node {
                    id: function.id.clone(),
                    name: SmolStr::new(function.name.clone()),
                    kind,
                    file: input.file_id,
                    span: Span::line(line_number),
                    language: Language::Rust,
                });
                result.edges.push(Edge {
                    kind: EdgeKind::Contains,
                    source: parent_id,
                    target: function.id.clone(),
                    origin: EdgeOrigin::Static,
                    confidence: 1.0,
                });

                if trimmed.ends_with(';') {
                    update_containers(&mut container_stack, net_braces);
                    continue;
                }

                let mut capture = FunctionCapture {
                    id: function.id,
                    module_path: SmolStr::new(module_path.clone()),
                    body: String::new(),
                    brace_balance: net_braces,
                };
                capture.body.push_str(raw_line);
                capture.body.push('\n');
                if capture.brace_balance <= 0 {
                    finalize_function(&mut result, capture);
                } else {
                    function_capture = Some(capture);
                }
                update_containers(&mut container_stack, net_braces);
                continue;
            }

            update_containers(&mut container_stack, net_braces);
        }

        Ok(result)
    }
}

#[derive(Clone)]
struct FunctionCapture {
    id: NodeId,
    module_path: SmolStr,
    body: String,
    brace_balance: i32,
}

struct ParsedFunction {
    id: NodeId,
    name: String,
    kind: NodeKind,
    parent: Option<NodeId>,
}

#[derive(Clone)]
struct Container {
    kind: ContainerKind,
    id: NodeId,
    brace_balance: i32,
}

impl Container {
    fn new(kind: ContainerKind, id: NodeId, brace_balance: i32) -> Self {
        Self {
            kind,
            id,
            brace_balance,
        }
    }
}

#[derive(Clone)]
enum ContainerKind {
    Impl(String),
    Trait(String),
}

fn parse_function(
    input: &ParseInput<'_>,
    module_path: &str,
    line_number: usize,
    line: &str,
    container: Option<&Container>,
) -> Option<ParsedFunction> {
    let name = captures(
        line,
        r"^(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe)\s+)*fn\s+([A-Za-z_][A-Za-z0-9_]*)",
    )?;
    let (kind, path, parent) = match container {
        Some(Container {
            kind: ContainerKind::Impl(target),
            id,
            ..
        }) => (
            NodeKind::Method,
            format!("{module_path}::{target}::{name}"),
            Some(id.clone()),
        ),
        Some(Container {
            kind: ContainerKind::Trait(target),
            id,
            ..
        }) => (
            NodeKind::Method,
            format!("{module_path}::{target}::{name}"),
            Some(id.clone()),
        ),
        None => (
            NodeKind::Function,
            format!("{module_path}::{name}"),
            Some(NodeId::new(
                input.crate_name,
                module_path.to_owned(),
                NodeKind::Module,
            )),
        ),
    };

    let _ = line_number;
    Some(ParsedFunction {
        id: NodeId::new(input.crate_name, path, kind),
        name,
        kind,
        parent,
    })
}

fn parse_struct(
    input: &ParseInput<'_>,
    module_path: &str,
    _line_number: usize,
    line: &str,
) -> Option<(NodeId, String)> {
    let name = captures(
        line,
        r"^(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)",
    )?;
    Some((
        NodeId::new(
            input.crate_name,
            format!("{module_path}::{name}"),
            NodeKind::Struct,
        ),
        name,
    ))
}

fn parse_enum(
    input: &ParseInput<'_>,
    module_path: &str,
    _line_number: usize,
    line: &str,
) -> Option<(NodeId, String)> {
    let name = captures(
        line,
        r"^(?:pub(?:\([^)]*\))?\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)",
    )?;
    Some((
        NodeId::new(
            input.crate_name,
            format!("{module_path}::{name}"),
            NodeKind::Enum,
        ),
        name,
    ))
}

fn parse_trait(
    input: &ParseInput<'_>,
    module_path: &str,
    _line_number: usize,
    line: &str,
) -> Option<(NodeId, String)> {
    let name = captures(
        line,
        r"^(?:pub(?:\([^)]*\))?\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)",
    )?;
    Some((
        NodeId::new(
            input.crate_name,
            format!("{module_path}::{name}"),
            NodeKind::Trait,
        ),
        name,
    ))
}

fn parse_type_alias(
    input: &ParseInput<'_>,
    module_path: &str,
    _line_number: usize,
    line: &str,
) -> Option<(NodeId, String)> {
    let name = captures(
        line,
        r"^(?:pub(?:\([^)]*\))?\s+)?type\s+([A-Za-z_][A-Za-z0-9_]*)",
    )?;
    Some((
        NodeId::new(
            input.crate_name,
            format!("{module_path}::{name}"),
            NodeKind::TypeAlias,
        ),
        name,
    ))
}

fn parse_impl(
    input: &ParseInput<'_>,
    module_path: &str,
    _line_number: usize,
    line: &str,
) -> Option<(NodeId, String, String)> {
    let rest = captures(line, r"^impl(?:<[^>]+>)?\s+(.+?)\s*\{")?;
    let (path_suffix, label, target_name) = canonical_impl_parts(&rest);
    Some((
        NodeId::new(
            input.crate_name,
            format!("{module_path}::{path_suffix}"),
            NodeKind::Impl,
        ),
        label,
        target_name,
    ))
}

fn canonical_impl_parts(value: &str) -> (String, String, String) {
    let compact = value.replace(' ', "");
    if let Some((trait_name, target_name)) = compact.split_once("for") {
        let trait_name = normalize_type_name(trait_name);
        let target_name = normalize_type_name(target_name);
        (
            format!("{target_name}::impl::{trait_name}"),
            format!("{trait_name} for {target_name}"),
            target_name,
        )
    } else {
        let target_name = normalize_type_name(&compact);
        (
            format!("{target_name}::impl"),
            target_name.clone(),
            target_name,
        )
    }
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
}

fn finalize_function(result: &mut ParseResult, function: FunctionCapture) {
    for call in extract_calls(&function.body) {
        result.unresolved_calls.push(UnresolvedCall {
            source: function.id.clone(),
            name: SmolStr::new(call),
            module_path: function.module_path.clone(),
        });
    }
}

fn extract_calls(body: &str) -> Vec<String> {
    let body = body.split_once('{').map(|(_, rest)| rest).unwrap_or(body);
    let mut calls = Vec::new();
    let call_re = Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();
    let method_re = Regex::new(r"\.\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();

    for captures in call_re.captures_iter(body) {
        let name = captures.get(1).unwrap().as_str();
        if is_keyword(name) {
            continue;
        }
        calls.push(name.to_owned());
    }

    for captures in method_re.captures_iter(body) {
        let name = captures.get(1).unwrap().as_str();
        if is_keyword(name) {
            continue;
        }
        calls.push(name.to_owned());
    }

    calls.sort();
    calls.dedup();
    calls
}

fn is_keyword(value: &str) -> bool {
    matches!(
        value,
        "if" | "match"
            | "loop"
            | "while"
            | "for"
            | "Some"
            | "Ok"
            | "Err"
            | "Self"
            | "self"
            | "let"
            | "return"
    )
}

fn push_node(
    result: &mut ParseResult,
    id: NodeId,
    name: String,
    kind: NodeKind,
    input: &ParseInput<'_>,
    line_number: usize,
) {
    result.nodes.push(Node {
        id,
        name: SmolStr::new(name),
        kind,
        file: input.file_id,
        span: Span::line(line_number),
        language: Language::Rust,
    });
}

fn add_contains_edge(
    result: &mut ParseResult,
    module_id: &NodeId,
    stack: &[Container],
    _kind: NodeKind,
) {
    let source = stack
        .last()
        .map(|container| container.id.clone())
        .unwrap_or_else(|| module_id.clone());
    let target = result.nodes.last().unwrap().id.clone();
    result.edges.push(Edge {
        kind: EdgeKind::Contains,
        source,
        target,
        origin: EdgeOrigin::Static,
        confidence: 1.0,
    });
}

fn update_containers(stack: &mut Vec<Container>, net_braces: i32) {
    for container in stack.iter_mut() {
        container.brace_balance += net_braces;
    }
    pop_closed_containers(stack);
}

fn pop_closed_containers(stack: &mut Vec<Container>) {
    while stack
        .last()
        .map_or(false, |container| container.brace_balance <= 0)
    {
        stack.pop();
    }
}

fn count_braces(line: &str) -> i32 {
    let mut balance = 0;
    for ch in line.chars() {
        match ch {
            '{' => balance += 1,
            '}' => balance -= 1,
            _ => {}
        }
    }
    balance
}

fn strip_comment(line: &str) -> String {
    line.split("//").next().unwrap_or(line).to_owned()
}

fn captures(line: &str, pattern: &str) -> Option<String> {
    Regex::new(pattern)
        .ok()?
        .captures(line)?
        .get(1)
        .map(|value| value.as_str().to_owned())
}

fn module_path(input: &ParseInput<'_>) -> String {
    let relative = relative_file(input);
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
            crate_name: "demo",
            workspace_root: Path::new("/workspace"),
            path: Path::new("/workspace/src/lib.rs"),
            file_id: FileId(1),
            source: "fn alpha() { beta(); }\nfn beta() {}\n",
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Function && node.name == "alpha"));
        assert!(result
            .unresolved_calls
            .iter()
            .any(|call| call.name == "beta"));
        assert!(!result
            .unresolved_calls
            .iter()
            .any(|call| call.name == "alpha"));
    }
}
