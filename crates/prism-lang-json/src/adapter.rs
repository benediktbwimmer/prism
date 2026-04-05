use anyhow::{Context, Result};
use prism_ir::{
    Edge, EdgeKind, EdgeOrigin, Language, Node, NodeId, NodeKind, Span, UnresolvedIntent,
};
use prism_parser::{
    document_name, document_path, extract_intent_targets, fingerprint_from_parts,
    intent_kind_for_context, normalized_shape_hash, whole_file_span, LanguageAdapter, ParseInput,
    ParseResult,
};
use serde_json::Value;
use smol_str::SmolStr;
use std::collections::HashMap;

pub struct JsonAdapter;

impl LanguageAdapter for JsonAdapter {
    fn language(&self) -> Language {
        Language::Json
    }

    fn supports_path(&self, path: &std::path::Path) -> bool {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("json"))
    }

    fn parse(&self, input: &ParseInput<'_>) -> Result<ParseResult> {
        let value = parse_json_document(input.source)
            .with_context(|| format!("failed to parse JSON file {}", input.path.display()))?;
        let mut result = ParseResult::default();
        let document_path = document_path(input);
        let document_id = NodeId::new(input.crate_name, document_path.clone(), NodeKind::Document);
        let document_shape = normalized_shape_hash(input.source);
        let document_node = Node {
            id: document_id.clone(),
            name: SmolStr::new(document_name(input)),
            kind: NodeKind::Document,
            file: input.file_id,
            span: Span::whole_file(input.source.len()),
            language: Language::Json,
        };
        result.record_fingerprint(
            &document_id,
            fingerprint_from_parts(["json", "document", document_shape.as_str()]),
        );
        result.nodes.push(document_node);
        let spans = JsonSpanIndex::new(input.source);
        walk_value(
            input,
            &mut result,
            &value,
            Some(document_id),
            &document_path,
            input.crate_name,
            &spans,
            Vec::new(),
        );
        Ok(result)
    }
}

fn parse_json_document(source: &str) -> Result<Value> {
    match serde_json::from_str(source) {
        Ok(value) => Ok(value),
        Err(_) => {
            let without_comments = strip_json_comments(source);
            let normalized = strip_trailing_commas(&without_comments);
            serde_json::from_str(&normalized).context("input is not valid JSON or JSONC")
        }
    }
}

fn strip_json_comments(source: &str) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escape = false;

    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
            output.push(ch);
            index += 1;
            continue;
        }

        if ch == '/' && index + 1 < chars.len() {
            match chars[index + 1] {
                '/' => {
                    output.push(' ');
                    output.push(' ');
                    index += 2;
                    while index < chars.len() {
                        let comment_ch = chars[index];
                        if comment_ch == '\n' {
                            output.push('\n');
                            index += 1;
                            break;
                        }
                        output.push(if comment_ch == '\r' { '\r' } else { ' ' });
                        index += 1;
                    }
                    continue;
                }
                '*' => {
                    output.push(' ');
                    output.push(' ');
                    index += 2;
                    while index < chars.len() {
                        let comment_ch = chars[index];
                        if comment_ch == '*' && index + 1 < chars.len() && chars[index + 1] == '/' {
                            output.push(' ');
                            output.push(' ');
                            index += 2;
                            break;
                        }
                        output.push(if matches!(comment_ch, '\n' | '\r') {
                            comment_ch
                        } else {
                            ' '
                        });
                        index += 1;
                    }
                    continue;
                }
                _ => {}
            }
        }

        output.push(ch);
        index += 1;
    }

    output
}

fn strip_trailing_commas(source: &str) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(source.len());
    let mut in_string = false;
    let mut escape = false;

    for (index, ch) in chars.iter().copied().enumerate() {
        if in_string {
            output.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            output.push(ch);
            continue;
        }

        if ch == ',' {
            let mut lookahead = index + 1;
            while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            if lookahead < chars.len() && matches!(chars[lookahead], ']' | '}') {
                output.push(' ');
                continue;
            }
        }

        output.push(ch);
    }

    output
}

fn walk_value(
    input: &ParseInput<'_>,
    result: &mut ParseResult,
    value: &Value,
    parent: Option<NodeId>,
    prefix: &str,
    crate_name: &str,
    spans: &JsonSpanIndex,
    key_path: Vec<String>,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = format!("{prefix}::{key}");
                let id = NodeId::new(crate_name, path.clone(), NodeKind::JsonKey);
                let value_shape = value_shape(child);
                let mut current_key_path = key_path.clone();
                current_key_path.push(key.clone());
                let span = spans
                    .span_for_path(&current_key_path)
                    .unwrap_or_else(|| whole_file_span(input.source));
                let node = Node {
                    id: id.clone(),
                    name: SmolStr::new(key),
                    kind: NodeKind::JsonKey,
                    file: input.file_id,
                    span,
                    language: Language::Json,
                };
                result.record_fingerprint(
                    &id,
                    fingerprint_from_parts(["json", "key", value_shape.as_str()]),
                );
                result.nodes.push(node);
                if let Some(parent) = &parent {
                    result.edges.push(Edge {
                        kind: EdgeKind::Contains,
                        source: parent.clone(),
                        target: id.clone(),
                        origin: EdgeOrigin::Static,
                        confidence: 1.0,
                    });
                }
                let intent_kind = intent_kind_for_context(&path, EdgeKind::RelatedTo);
                for target in intent_targets_for_value(key, child) {
                    result.unresolved_intents.push(UnresolvedIntent {
                        source: id.clone(),
                        kind: intent_kind,
                        target: target.into(),
                        span,
                    });
                }
                walk_value(
                    input,
                    result,
                    child,
                    Some(id),
                    &path,
                    crate_name,
                    spans,
                    current_key_path,
                );
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let path = format!("{prefix}::[{index}]");
                walk_value(
                    input,
                    result,
                    child,
                    parent.clone(),
                    &path,
                    crate_name,
                    spans,
                    key_path.clone(),
                );
            }
        }
        _ => {}
    }
}

fn value_shape(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "bool".to_owned(),
        Value::Number(_) => "number".to_owned(),
        Value::String(_) => "string".to_owned(),
        Value::Array(values) => format!("array:{}", values.len()),
        Value::Object(map) => format!("object:{}", map.len()),
    }
}

fn intent_targets_for_value(key: &str, value: &Value) -> Vec<String> {
    let mut targets = extract_intent_targets(key);
    collect_value_targets(value, &mut targets);
    targets.sort();
    targets.dedup();
    targets
}

fn collect_value_targets(value: &Value, targets: &mut Vec<String>) {
    match value {
        Value::String(text) => targets.extend(extract_intent_targets(text)),
        Value::Array(values) => {
            for value in values {
                collect_value_targets(value, targets);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                targets.extend(extract_intent_targets(key));
                collect_value_targets(value, targets);
            }
        }
        _ => {}
    }
}

struct JsonSpanIndex {
    spans: HashMap<String, Span>,
}

impl JsonSpanIndex {
    fn new(source: &str) -> Self {
        let mut index = JsonSpanIndexer {
            source,
            offset: 0,
            spans: HashMap::new(),
        };
        index.skip_ws_and_comments();
        index.parse_value(Vec::new());
        Self { spans: index.spans }
    }

    fn span_for_path(&self, path: &[String]) -> Option<Span> {
        self.spans.get(&path.join("::")).copied()
    }
}

struct JsonSpanIndexer<'a> {
    source: &'a str,
    offset: usize,
    spans: HashMap<String, Span>,
}

impl<'a> JsonSpanIndexer<'a> {
    fn parse_value(&mut self, path: Vec<String>) {
        self.skip_ws_and_comments();
        match self.peek_byte() {
            Some(b'{') => self.parse_object(path),
            Some(b'[') => self.parse_array(path),
            Some(b'"') => {
                let _ = self.parse_string();
            }
            Some(_) => self.parse_scalar(),
            None => {}
        }
    }

    fn parse_object(&mut self, path: Vec<String>) {
        self.offset += 1;
        loop {
            self.skip_ws_and_comments();
            if matches!(self.peek_byte(), Some(b'}')) {
                self.offset += 1;
                break;
            }
            let Some((key, span)) = self.parse_string() else {
                break;
            };
            let mut child_path = path.clone();
            child_path.push(key);
            self.spans.entry(child_path.join("::")).or_insert(span);
            self.skip_ws_and_comments();
            if matches!(self.peek_byte(), Some(b':')) {
                self.offset += 1;
            }
            self.parse_value(child_path);
            self.skip_ws_and_comments();
            match self.peek_byte() {
                Some(b',') => {
                    self.offset += 1;
                }
                Some(b'}') => {
                    self.offset += 1;
                    break;
                }
                _ => break,
            }
        }
    }

    fn parse_array(&mut self, path: Vec<String>) {
        self.offset += 1;
        loop {
            self.skip_ws_and_comments();
            if matches!(self.peek_byte(), Some(b']')) {
                self.offset += 1;
                break;
            }
            self.parse_value(path.clone());
            self.skip_ws_and_comments();
            match self.peek_byte() {
                Some(b',') => {
                    self.offset += 1;
                }
                Some(b']') => {
                    self.offset += 1;
                    break;
                }
                _ => break,
            }
        }
    }

    fn parse_scalar(&mut self) {
        while let Some(byte) = self.peek_byte() {
            if matches!(byte, b',' | b']' | b'}') || byte.is_ascii_whitespace() {
                break;
            }
            self.offset += 1;
        }
    }

    fn parse_string(&mut self) -> Option<(String, Span)> {
        if self.peek_byte()? != b'"' {
            return None;
        }
        let quote_start = self.offset;
        self.offset += 1;
        let content_start = self.offset;
        let mut value = String::new();
        let mut escape = false;
        while let Some((ch, next_offset)) = next_char(self.source, self.offset) {
            if escape {
                value.push(ch);
                escape = false;
                self.offset = next_offset;
                continue;
            }
            match ch {
                '\\' => {
                    escape = true;
                    self.offset = next_offset;
                }
                '"' => {
                    let span = Span::new(content_start, self.offset);
                    self.offset = next_offset;
                    return Some((value, span));
                }
                _ => {
                    value.push(ch);
                    self.offset = next_offset;
                }
            }
        }
        self.offset = quote_start;
        None
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while let Some(byte) = self.peek_byte() {
                if byte.is_ascii_whitespace() {
                    self.offset += 1;
                } else {
                    break;
                }
            }
            let Some(byte) = self.peek_byte() else {
                break;
            };
            let next = self.source.as_bytes().get(self.offset + 1).copied();
            if byte == b'/' && next == Some(b'/') {
                self.offset += 2;
                while let Some(current) = self.peek_byte() {
                    self.offset += 1;
                    if current == b'\n' {
                        break;
                    }
                }
                continue;
            }
            if byte == b'/' && next == Some(b'*') {
                self.offset += 2;
                while self.offset + 1 < self.source.len() {
                    if self.source.as_bytes()[self.offset] == b'*'
                        && self.source.as_bytes()[self.offset + 1] == b'/'
                    {
                        self.offset += 2;
                        break;
                    }
                    self.offset += 1;
                }
                continue;
            }
            break;
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.offset).copied()
    }
}

fn next_char(source: &str, index: usize) -> Option<(char, usize)> {
    source[index..]
        .chars()
        .next()
        .map(|ch| (ch, index + ch.len_utf8()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use prism_ir::{FileId, NodeKind};
    use prism_parser::{LanguageAdapter, ParseDepth, ParseInput};

    use super::JsonAdapter;

    #[test]
    fn parses_document_anchor_and_keys() {
        let adapter = JsonAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("workspace"),
            path: Path::new("workspace/config/app.json"),
            file_id: FileId(1),
            parse_depth: ParseDepth::Deep,
            source: "{\n  \"service\": {\n    \"port\": 8080\n  }\n}",
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::Document));
        let service = result
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::JsonKey && node.name == "service")
            .unwrap();
        assert_eq!(
            &input.source[service.span.start as usize..service.span.end as usize],
            "service"
        );
        assert_eq!(result.edges.len(), 2);
    }

    #[test]
    fn parses_jsonc_with_comments_and_trailing_commas() {
        let adapter = JsonAdapter;
        let input = ParseInput {
            package_name: "demo",
            crate_name: "demo",
            package_root: Path::new("workspace"),
            path: Path::new("workspace/config/tsconfig.json"),
            file_id: FileId(1),
            parse_depth: ParseDepth::Deep,
            source: r#"{
  "compilerOptions": {
    /* Bundler mode */
    "moduleResolution": "bundler",
    "types": [
      "vite/client",
    ],
  },
}"#,
        };

        let result = adapter.parse(&input).unwrap();
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::JsonKey && node.name == "compilerOptions"));
        assert!(result
            .nodes
            .iter()
            .any(|node| node.kind == NodeKind::JsonKey && node.name == "moduleResolution"));
    }
}
