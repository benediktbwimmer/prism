use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context as AnyhowContext, Result};
use deno_ast::{
    parse_program, EmitOptions, MediaType, ModuleSpecifier, ParseParams, TranspileModuleOptions,
    TranspileOptions,
};
use prism_core::index_workspace;
use prism_ir::{NodeId, NodeKind};
use prism_js::{
    api_reference_markdown, runtime_prelude, LineageView, QueryDiagnostic, QueryEnvelope,
    RelationsView, SymbolView, API_REFERENCE_URI,
};
use prism_query::{Prism, Symbol};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use rquickjs::{prelude::Func, Context, Runtime};
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 50;
const DEFAULT_CALL_GRAPH_DEPTH: usize = 3;
const MAX_CALL_GRAPH_DEPTH: usize = 5;
const MAX_ENTRYPOINTS: usize = 100;

#[derive(Debug, Clone, clap::Parser)]
#[command(name = "prism-mcp")]
#[command(about = "MCP server for programmable PRISM queries")]
pub struct PrismMcpCli {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
}

#[derive(Clone)]
pub struct PrismMcpServer {
    tool_router: ToolRouter<PrismMcpServer>,
    host: Arc<QueryHost>,
}

impl PrismMcpServer {
    pub fn from_workspace(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let prism = index_workspace(&root)?;
        Ok(Self::new(prism))
    }

    pub fn new(prism: Prism) -> Self {
        Self {
            tool_router: Self::tool_router(),
            host: Arc::new(QueryHost::new(prism)),
        }
    }

    pub async fn serve_stdio(self) -> Result<()> {
        let service = self.serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum QueryLanguage {
    Ts,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PrismQueryArgs {
    #[schemars(description = "TypeScript snippet evaluated with a global `prism` object.")]
    code: String,
    #[schemars(description = "Query language. Only `ts` is currently supported.")]
    language: Option<QueryLanguage>,
}

#[tool_router]
impl PrismMcpServer {
    #[tool(
        name = "prism_query",
        description = "Execute a read-only TypeScript query against the live PRISM graph. Read prism://api-reference for the available prism API."
    )]
    fn prism_query(
        &self,
        Parameters(args): Parameters<PrismQueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.code.trim().is_empty() {
            return Err(McpError::invalid_params(
                "query code cannot be empty",
                Some(json!({ "field": "code" })),
            ));
        }

        let language = args.language.unwrap_or(QueryLanguage::Ts);
        let envelope = self
            .host
            .execute(&args.code, language)
            .map_err(map_query_error)?;
        let content = Content::json(envelope).map_err(|err| {
            McpError::internal_error(
                "failed to serialize query result",
                Some(json!({ "error": err.to_string() })),
            )
        })?;
        Ok(CallToolResult::success(vec![content]))
    }
}

#[tool_handler]
impl ServerHandler for PrismMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_instructions(
            "Use the prism_query tool for read-only programmable graph queries and read prism://api-reference for the typed PRISM query surface.",
        )
        .with_protocol_version(ProtocolVersion::LATEST)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![RawResource::new(API_REFERENCE_URI, "PRISM API Reference")
                .with_description("TypeScript query surface, d.ts-style contract, and recipes")
                .no_annotation()],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        if request.uri.as_str() != API_REFERENCE_URI {
            return Err(McpError::resource_not_found(
                "resource_not_found",
                Some(json!({ "uri": request.uri })),
            ));
        }

        Ok(ReadResourceResult::new(vec![ResourceContents::text(
            api_reference_markdown(),
            request.uri,
        )]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            next_cursor: None,
            resource_templates: Vec::new(),
            meta: None,
        })
    }
}

fn map_query_error(error: anyhow::Error) -> McpError {
    McpError::internal_error(
        "prism query failed",
        Some(json!({
            "code": "query_execution_failed",
            "error": error.to_string(),
        })),
    )
}

#[derive(Clone)]
struct QueryHost {
    prism: Arc<Prism>,
    worker: Arc<JsWorker>,
}

impl QueryHost {
    fn new(prism: Prism) -> Self {
        let prism = Arc::new(prism);
        Self {
            prism: prism.clone(),
            worker: Arc::new(JsWorker::spawn()),
        }
    }

    fn execute(&self, code: &str, language: QueryLanguage) -> Result<QueryEnvelope> {
        match language {
            QueryLanguage::Ts => self.execute_typescript(code),
        }
    }

    fn execute_typescript(&self, code: &str) -> Result<QueryEnvelope> {
        let source = format!(
            "(function() {{\n  const __prismUserQuery = () => {{\n{}\n  }};\n  const __prismResult = __prismUserQuery();\n  return __prismResult === undefined ? \"null\" : JSON.stringify(__prismResult);\n}})();\n",
            code
        );
        let transpiled = transpile_typescript(&source)?;
        let execution = QueryExecution::new(self.clone());
        let raw_result = self.worker.execute(transpiled, execution.clone())?;
        let result =
            serde_json::from_str(&raw_result).context("failed to decode query result JSON")?;
        Ok(QueryEnvelope {
            result,
            diagnostics: execution.diagnostics(),
        })
    }

    fn symbols(&self, query: &str) -> Result<Vec<SymbolView>> {
        self.prism
            .symbol(query)
            .iter()
            .map(|symbol| self.symbol_view(symbol))
            .collect()
    }

    fn entrypoints(&self) -> Result<Vec<SymbolView>> {
        self.prism
            .entrypoints()
            .iter()
            .map(|symbol| self.symbol_view(symbol))
            .collect()
    }

    fn relations(&self, id: &NodeId) -> Result<RelationsView> {
        let relations = self.symbol_for(id)?.relations();
        Ok(RelationsView {
            outgoing_calls: relations.outgoing_calls,
            incoming_calls: relations.incoming_calls,
            outgoing_imports: relations.outgoing_imports,
            incoming_imports: relations.incoming_imports,
            outgoing_implements: relations.outgoing_implements,
            incoming_implements: relations.incoming_implements,
        })
    }

    fn lineage(&self, id: &NodeId) -> Result<Option<LineageView>> {
        let Some(lineage) = self.prism.lineage_of(id) else {
            return Ok(None);
        };
        Ok(Some(LineageView {
            events: self.prism.lineage_history(&lineage),
            lineage,
        }))
    }

    fn symbol_view(&self, symbol: &Symbol<'_>) -> Result<SymbolView> {
        let node = symbol.node();
        Ok(SymbolView {
            id: symbol.id().clone(),
            name: symbol.name().to_owned(),
            kind: node.kind,
            signature: symbol.signature(),
            file_path: self
                .prism
                .graph()
                .file_path(node.file)
                .map(|path| path.to_string_lossy().into_owned()),
            span: node.span,
            language: node.language,
            lineage_id: self.prism.lineage_of(symbol.id()),
        })
    }

    fn symbol_for<'a>(&'a self, id: &NodeId) -> Result<Symbol<'a>> {
        let node = self
            .prism
            .graph()
            .node(id)
            .ok_or_else(|| anyhow!("unknown symbol `{}`", id.path))?;
        let matching = self.prism.search(
            &node.id.path,
            self.prism.graph().nodes.len().max(1),
            Some(node.kind),
            None,
        );
        matching
            .into_iter()
            .find(|symbol| symbol.id() == id)
            .ok_or_else(|| anyhow!("symbol `{}` is no longer queryable", id.path))
    }
}

struct JsWorker {
    tx: mpsc::Sender<JsWorkerMessage>,
}

struct JsWorkerRequest {
    script: String,
    execution: QueryExecution,
    reply: mpsc::Sender<Result<String>>,
}

enum JsWorkerMessage {
    Execute(JsWorkerRequest),
}

impl JsWorker {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<JsWorkerMessage>();
        thread::spawn(move || {
            if let Err(error) = run_js_worker(rx) {
                eprintln!("prism-mcp js worker failed: {error}");
            }
        });
        Self { tx }
    }

    fn execute(&self, script: String, execution: QueryExecution) -> Result<String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(JsWorkerMessage::Execute(JsWorkerRequest {
                script,
                execution,
                reply: reply_tx,
            }))
            .map_err(|_| anyhow!("js worker is unavailable"))?;

        reply_rx
            .recv()
            .map_err(|_| anyhow!("js worker dropped the query response"))?
    }
}

fn run_js_worker(rx: mpsc::Receiver<JsWorkerMessage>) -> Result<()> {
    let runtime = Runtime::new().context("failed to create JS runtime")?;
    let context = Context::full(&runtime).context("failed to create JS context")?;
    let active_execution = Arc::new(Mutex::new(None::<QueryExecution>));

    context.with(|ctx| -> Result<()> {
        let current = active_execution.clone();
        ctx.globals().set(
            "__prismHostCall",
            Func::from(move |operation: String, args_json: String| {
                let execution = {
                    let guard = current.lock().expect("active execution lock poisoned");
                    guard.clone()
                };
                let Some(execution) = execution else {
                    return json!({
                        "ok": false,
                        "error": "no active prism query execution"
                    })
                    .to_string();
                };
                execution.dispatch_enveloped(&operation, &args_json)
            }),
        )?;
        ctx.eval::<(), _>(runtime_prelude())
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    })?;

    while let Ok(message) = rx.recv() {
        match message {
            JsWorkerMessage::Execute(request) => {
                {
                    let mut guard = active_execution
                        .lock()
                        .expect("active execution lock poisoned");
                    *guard = Some(request.execution.clone());
                }

                let result = context.with(|ctx| -> Result<String> {
                    ctx.eval::<String, _>(request.script.as_str())
                        .map_err(|err| anyhow!(err.to_string()))
                });

                {
                    let mut guard = active_execution
                        .lock()
                        .expect("active execution lock poisoned");
                    *guard = None;
                }

                let _ = request.reply.send(result);
            }
        }
    }

    Ok(())
}

#[derive(Clone)]
struct QueryExecution {
    host: QueryHost,
    diagnostics: Arc<Mutex<Vec<QueryDiagnostic>>>,
}

impl QueryExecution {
    fn new(host: QueryHost) -> Self {
        Self {
            host,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn diagnostics(&self) -> Vec<QueryDiagnostic> {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .clone()
    }

    fn push_diagnostic(&self, code: &str, message: impl Into<String>, data: Option<Value>) {
        self.diagnostics
            .lock()
            .expect("diagnostics lock poisoned")
            .push(QueryDiagnostic {
                code: code.to_owned(),
                message: message.into(),
                data,
            });
    }

    fn dispatch_enveloped(&self, operation: &str, args_json: &str) -> String {
        match self.dispatch(operation, args_json) {
            Ok(value) => json!({ "ok": true, "value": value }).to_string(),
            Err(error) => json!({ "ok": false, "error": error.to_string() }).to_string(),
        }
    }

    fn dispatch(&self, operation: &str, args_json: &str) -> Result<Value> {
        let args = if args_json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(args_json).context("failed to parse host-call arguments")?
        };

        match operation {
            "symbol" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.best_symbol(&args.query)?)?)
            }
            "symbols" => {
                let args: SymbolQueryArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.symbols(&args.query)?)?)
            }
            "search" => {
                let args: SearchArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.search(args)?)?)
            }
            "entrypoints" => Ok(serde_json::to_value(self.entrypoints()?)?),
            "full" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(
                    self.host.symbol_for(&args.id)?.full(),
                )?)
            }
            "relations" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.relations(&args.id)?)?)
            }
            "callGraph" => {
                let args: CallGraphArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.call_graph(args)?)?)
            }
            "lineage" => {
                let args: SymbolTargetArgs = serde_json::from_value(args)?;
                Ok(serde_json::to_value(self.host.lineage(&args.id)?)?)
            }
            "diagnostics" => Ok(serde_json::to_value(self.diagnostics())?),
            other => {
                self.push_diagnostic(
                    "unknown_method",
                    format!("Unknown Prism host operation `{other}`."),
                    Some(json!({ "operation": other })),
                );
                Err(anyhow!("unsupported host operation `{other}`"))
            }
        }
    }

    fn best_symbol(&self, query: &str) -> Result<Option<SymbolView>> {
        let matches = self.host.symbols(query)?;
        if matches.is_empty() {
            self.push_diagnostic(
                "anchor_unresolved",
                format!("No symbol matched `{query}`."),
                Some(json!({ "query": query })),
            );
            return Ok(None);
        }
        if matches.len() > 1 {
            self.push_diagnostic(
                "ambiguous_symbol",
                format!(
                    "`{query}` matched {} symbols; returning the first best match.",
                    matches.len()
                ),
                Some(json!({
                    "query": query,
                    "matches": matches
                        .iter()
                        .map(|symbol| symbol.id.path.to_string())
                        .collect::<Vec<_>>(),
                })),
            );
        }
        Ok(matches.into_iter().next())
    }

    fn search(&self, args: SearchArgs) -> Result<Vec<SymbolView>> {
        let kind = args.kind.as_deref().map(parse_node_kind).transpose()?;
        let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        let applied = requested.min(MAX_SEARCH_LIMIT);

        if requested > MAX_SEARCH_LIMIT {
            self.push_diagnostic(
                "result_truncated",
                format!("Search limit was capped at {MAX_SEARCH_LIMIT} instead of {requested}."),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }

        let mut results = self
            .host
            .prism
            .search(
                &args.query,
                applied.saturating_add(1),
                kind,
                args.path.as_deref(),
            )
            .iter()
            .map(|symbol| self.host.symbol_view(symbol))
            .collect::<Result<Vec<_>>>()?;

        if results.len() > applied {
            results.truncate(applied);
            self.push_diagnostic(
                "result_truncated",
                format!(
                    "Search results for `{}` were truncated at {} entries.",
                    args.query, applied
                ),
                Some(json!({
                    "query": args.query,
                    "applied": applied,
                })),
            );
        }

        Ok(results)
    }

    fn entrypoints(&self) -> Result<Vec<SymbolView>> {
        let mut results = self.host.entrypoints()?;
        if results.len() > MAX_ENTRYPOINTS {
            results.truncate(MAX_ENTRYPOINTS);
            self.push_diagnostic(
                "result_truncated",
                format!("Entrypoints were truncated at {} entries.", MAX_ENTRYPOINTS),
                Some(json!({
                    "applied": MAX_ENTRYPOINTS,
                })),
            );
        }
        Ok(results)
    }

    fn call_graph(&self, args: CallGraphArgs) -> Result<prism_ir::Subgraph> {
        let requested = args.depth.unwrap_or(DEFAULT_CALL_GRAPH_DEPTH);
        let applied = requested.min(MAX_CALL_GRAPH_DEPTH);
        if requested > MAX_CALL_GRAPH_DEPTH {
            self.push_diagnostic(
                "depth_limited",
                format!(
                    "Call-graph depth was capped at {MAX_CALL_GRAPH_DEPTH} instead of {requested}."
                ),
                Some(json!({
                    "requested": requested,
                    "applied": applied,
                })),
            );
        }
        Ok(self.host.symbol_for(&args.id)?.call_graph(applied))
    }
}

#[derive(Debug, Deserialize)]
struct SymbolQueryArgs {
    query: String,
}

#[derive(Debug, Deserialize)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
    kind: Option<String>,
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SymbolTargetArgs {
    id: NodeId,
}

#[derive(Debug, Deserialize)]
struct CallGraphArgs {
    id: NodeId,
    depth: Option<usize>,
}

fn parse_node_kind(value: &str) -> Result<NodeKind> {
    let normalized = value.trim().to_ascii_lowercase();
    let kind = match normalized.as_str() {
        "workspace" => NodeKind::Workspace,
        "package" => NodeKind::Package,
        "document" => NodeKind::Document,
        "module" => NodeKind::Module,
        "function" => NodeKind::Function,
        "struct" => NodeKind::Struct,
        "enum" => NodeKind::Enum,
        "trait" => NodeKind::Trait,
        "impl" => NodeKind::Impl,
        "method" => NodeKind::Method,
        "field" => NodeKind::Field,
        "typealias" | "type-alias" => NodeKind::TypeAlias,
        "markdownheading" | "markdown-heading" => NodeKind::MarkdownHeading,
        "jsonkey" | "json-key" => NodeKind::JsonKey,
        "yamlkey" | "yaml-key" => NodeKind::YamlKey,
        other => return Err(anyhow!("unknown node kind `{other}`")),
    };
    Ok(kind)
}

fn transpile_typescript(source: &str) -> Result<String> {
    let specifier = ModuleSpecifier::parse("file:///prism/query.ts")?;
    let parsed = parse_program(ParseParams {
        specifier,
        text: source.into(),
        media_type: MediaType::TypeScript,
        capture_tokens: false,
        maybe_syntax: None,
        scope_analysis: false,
    })
    .map_err(|err| anyhow!(err.to_string()))?;
    let transpiled = parsed
        .transpile(
            &TranspileOptions::default(),
            &TranspileModuleOptions::default(),
            &EmitOptions::default(),
        )
        .map_err(|err| anyhow!(err.to_string()))?
        .into_source();
    Ok(transpiled.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::{Language, Node, NodeId, NodeKind, Span};
    use prism_store::Graph;
    use std::collections::HashMap;

    fn host_with_node(node: Node) -> QueryHost {
        let mut graph = Graph::default();
        graph.nodes.insert(node.id.clone(), node);
        graph.adjacency = HashMap::new();
        graph.reverse_adjacency = HashMap::new();
        QueryHost::new(Prism::new(graph))
    }

    fn demo_node() -> Node {
        Node {
            id: NodeId::new("demo", "demo::main", NodeKind::Function),
            name: "main".into(),
            kind: NodeKind::Function,
            file: prism_ir::FileId(1),
            span: Span::new(1, 1, 3, 1),
            language: Language::Rust,
        }
    }

    #[test]
    fn executes_symbol_query() {
        let host = host_with_node(demo_node());
        let result = host
            .execute(
                r#"
const sym = prism.symbol("main");
return { path: sym?.id.path, kind: sym?.kind };
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");
        assert_eq!(result.result["path"], "demo::main");
        assert_eq!(result.result["kind"], "Function");
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn search_kind_filter_uses_cli_style_names() {
        let host = host_with_node(demo_node());
        let result = host
            .execute(
                r#"
return prism.search("main", { kind: "function" });
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");
        assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
    }

    #[test]
    fn reports_diagnostics_for_overbroad_searches() {
        let host = host_with_node(demo_node());
        let result = host
            .execute(
                r#"
prism.search("main", { limit: 1000 });
return prism.diagnostics();
"#,
                QueryLanguage::Ts,
            )
            .expect("query should succeed");
        assert_eq!(result.result.as_array().map(|items| items.len()), Some(1));
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, "result_truncated");
    }

    #[test]
    fn reuses_warm_runtime_across_queries() {
        let host = host_with_node(demo_node());

        let first = host
            .execute(
                r#"
const sym = prism.symbol("main");
return sym?.id.path;
"#,
                QueryLanguage::Ts,
            )
            .expect("first query should succeed");
        let second = host
            .execute(
                r#"
return prism.entrypoints().map((sym) => sym.id.path);
"#,
                QueryLanguage::Ts,
            )
            .expect("second query should succeed");

        assert_eq!(first.result, Value::String("demo::main".to_owned()));
        assert_eq!(second.result.as_array().map(|items| items.len()), Some(1));
    }
}
