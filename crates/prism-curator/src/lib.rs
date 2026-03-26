use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use prism_agent::InferredEdgeScope;
use prism_ir::{AnchorRef, Edge, EventId, LineageEvent, Node, NodeId, TaskId};
use prism_memory::{MemoryEntry, MemoryKind, OutcomeEvent};
use prism_projections::{CoChangeRecord, ValidationCheck};
use serde::{Deserialize, Serialize};
use toml::Value as TomlValue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratorJobId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorTrigger {
    Manual,
    PostChange,
    TaskCompleted,
    RepeatedFailure,
    AmbiguousLineage,
    HotspotChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratorBudget {
    pub max_input_bytes: usize,
    pub max_context_nodes: usize,
    pub max_outcomes: usize,
    pub max_memories: usize,
    pub max_proposals: usize,
}

impl Default for CuratorBudget {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 * 1024,
            max_context_nodes: 128,
            max_outcomes: 64,
            max_memories: 32,
            max_proposals: 12,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorJob {
    pub id: CuratorJobId,
    pub trigger: CuratorTrigger,
    pub task: Option<TaskId>,
    pub focus: Vec<AnchorRef>,
    pub budget: CuratorBudget,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorGraphSlice {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorLineageSlice {
    pub events: Vec<LineageEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorProjectionSlice {
    pub co_change: Vec<CoChangeRecord>,
    pub validation_checks: Vec<ValidationCheck>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorContext {
    pub graph: CuratorGraphSlice,
    pub lineage: CuratorLineageSlice,
    pub outcomes: Vec<OutcomeEvent>,
    pub memories: Vec<MemoryEntry>,
    pub projections: CuratorProjectionSlice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateEdge {
    pub edge: Edge,
    pub scope: InferredEdgeScope,
    pub evidence: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateMemory {
    pub anchors: Vec<AnchorRef>,
    pub kind: MemoryKind,
    pub content: String,
    pub trust: f32,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateRiskSummary {
    pub anchors: Vec<AnchorRef>,
    pub summary: String,
    pub severity: String,
    pub evidence_events: Vec<EventId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateValidationRecipe {
    pub target: NodeId,
    pub checks: Vec<String>,
    pub rationale: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CuratorProposal {
    InferredEdge(CandidateEdge),
    StructuralMemory(CandidateMemory),
    RiskSummary(CandidateRiskSummary),
    ValidationRecipe(CandidateValidationRecipe),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorDiagnostic {
    pub code: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorRun {
    pub proposals: Vec<CuratorProposal>,
    pub diagnostics: Vec<CuratorDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorProposalDisposition {
    Pending,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorProposalState {
    pub disposition: CuratorProposalDisposition,
    pub decided_at: Option<u64>,
    pub task: Option<TaskId>,
    pub note: Option<String>,
    pub output: Option<String>,
}

impl Default for CuratorProposalState {
    fn default() -> Self {
        Self {
            disposition: CuratorProposalDisposition::Pending,
            decided_at: None,
            task: None,
            note: None,
            output: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorJobRecord {
    pub id: CuratorJobId,
    pub job: CuratorJob,
    pub status: CuratorJobStatus,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub run: Option<CuratorRun>,
    #[serde(default)]
    pub proposal_states: Vec<CuratorProposalState>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CuratorSnapshot {
    pub records: Vec<CuratorJobRecord>,
}

pub trait CuratorBackend: Send + Sync {
    fn run(&self, job: &CuratorJob, ctx: &CuratorContext) -> Result<CuratorRun>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexSandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl CodexSandboxMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexApprovalPolicy {
    Untrusted,
    OnFailure,
    OnRequest,
    Never,
}

impl CodexApprovalPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::OnFailure => "on-failure",
            Self::OnRequest => "on-request",
            Self::Never => "never",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexReasoningEffort {
    Minimal,
    None,
    Low,
    Medium,
    High,
    Xhigh,
}

impl CodexReasoningEffort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexLocalProvider {
    Lmstudio,
    Ollama,
}

impl CodexLocalProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lmstudio => "lmstudio",
            Self::Ollama => "ollama",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexExecutionMode {
    Standard,
    FullAuto,
    DangerousBypass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexConfigOverride {
    pub key: String,
    pub value: TomlValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexCliCuratorConfig {
    pub binary: PathBuf,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub sandbox: Option<CodexSandboxMode>,
    pub approval_policy: Option<CodexApprovalPolicy>,
    pub reasoning_effort: Option<CodexReasoningEffort>,
    pub execution_mode: CodexExecutionMode,
    pub cwd: PathBuf,
    pub add_dirs: Vec<PathBuf>,
    pub skip_git_repo_check: bool,
    pub ephemeral: bool,
    pub oss: bool,
    pub local_provider: Option<CodexLocalProvider>,
    pub enable_features: Vec<String>,
    pub disable_features: Vec<String>,
    pub config_overrides: Vec<CodexConfigOverride>,
}

impl CodexCliCuratorConfig {
    pub fn codex(binary: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            model: None,
            profile: None,
            sandbox: None,
            approval_policy: None,
            reasoning_effort: None,
            execution_mode: CodexExecutionMode::Standard,
            cwd: cwd.into(),
            add_dirs: Vec::new(),
            skip_git_repo_check: false,
            ephemeral: false,
            oss: false,
            local_provider: None,
            enable_features: Vec::new(),
            disable_features: Vec::new(),
            config_overrides: Vec::new(),
        }
    }

    fn all_overrides(&self) -> Vec<CodexConfigOverride> {
        let mut overrides = Vec::new();
        if let Some(policy) = self.approval_policy {
            overrides.push(CodexConfigOverride {
                key: "approval_policy".to_string(),
                value: TomlValue::String(policy.as_str().to_string()),
            });
        }
        if let Some(effort) = self.reasoning_effort {
            overrides.push(CodexConfigOverride {
                key: "reasoning_effort".to_string(),
                value: TomlValue::String(effort.as_str().to_string()),
            });
        }
        overrides.extend(self.config_overrides.clone());
        overrides
    }
}

pub struct CodexCliCurator {
    config: CodexCliCuratorConfig,
}

impl CodexCliCurator {
    pub fn new(config: CodexCliCuratorConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &CodexCliCuratorConfig {
        &self.config
    }

    fn prompt(&self, job: &CuratorJob, ctx: &CuratorContext) -> Result<String> {
        let bounded = bounded_context(ctx, &job.budget);
        let payload = serde_json::to_string_pretty(&serde_json::json!({
            "job": job,
            "context": bounded,
        }))?;
        if payload.len() > job.budget.max_input_bytes {
            bail!(
                "curator payload exceeded the configured {} byte budget",
                job.budget.max_input_bytes
            );
        }

        Ok(format!(
            "You are the PRISM background curator.\n\
Produce only JSON matching the provided schema.\n\
Do not mutate authoritative structure. Prefer no proposal over weak proposals.\n\
Use the job budget and evidence density to stay conservative.\n\
\n\
Allowed proposal kinds:\n\
- inferred_edge\n\
- structural_memory\n\
- risk_summary\n\
- validation_recipe\n\
\n\
JSON payload:\n\
{payload}\n"
        ))
    }

    fn prepare_invocation(
        &self,
        job: &CuratorJob,
        ctx: &CuratorContext,
    ) -> Result<PreparedCodexInvocation> {
        let temp_dir = unique_temp_dir("prism-curator")?;
        let schema_path = temp_dir.join("curator-run.schema.json");
        let output_path = temp_dir.join("curator-run.output.json");
        let prompt = self.prompt(job, ctx)?;

        fs::write(
            &schema_path,
            serde_json::to_vec_pretty(&curator_run_schema())?,
        )?;

        let mut args = vec!["exec".to_string()];
        if let Some(model) = &self.config.model {
            args.push("-m".to_string());
            args.push(model.clone());
        }
        if let Some(profile) = &self.config.profile {
            args.push("-p".to_string());
            args.push(profile.clone());
        }
        if let Some(sandbox) = self.config.sandbox {
            args.push("-s".to_string());
            args.push(sandbox.as_str().to_string());
        }
        match self.config.execution_mode {
            CodexExecutionMode::Standard => {}
            CodexExecutionMode::FullAuto => args.push("--full-auto".to_string()),
            CodexExecutionMode::DangerousBypass => {
                args.push("--dangerously-bypass-approvals-and-sandbox".to_string())
            }
        }
        args.push("-C".to_string());
        args.push(self.config.cwd.display().to_string());
        for dir in &self.config.add_dirs {
            args.push("--add-dir".to_string());
            args.push(dir.display().to_string());
        }
        if self.config.skip_git_repo_check {
            args.push("--skip-git-repo-check".to_string());
        }
        if self.config.ephemeral {
            args.push("--ephemeral".to_string());
        }
        if self.config.oss {
            args.push("--oss".to_string());
        }
        if let Some(provider) = self.config.local_provider {
            args.push("--local-provider".to_string());
            args.push(provider.as_str().to_string());
        }
        for feature in &self.config.enable_features {
            args.push("--enable".to_string());
            args.push(feature.clone());
        }
        for feature in &self.config.disable_features {
            args.push("--disable".to_string());
            args.push(feature.clone());
        }
        for override_value in self.config.all_overrides() {
            args.push("-c".to_string());
            args.push(format!(
                "{}={}",
                override_value.key,
                render_toml_value(&override_value.value)?
            ));
        }
        args.push("--output-schema".to_string());
        args.push(schema_path.display().to_string());
        args.push("-o".to_string());
        args.push(output_path.display().to_string());
        args.push("-".to_string());

        Ok(PreparedCodexInvocation {
            args,
            prompt,
            schema_path,
            output_path,
            temp_dir,
        })
    }
}

impl CuratorBackend for CodexCliCurator {
    fn run(&self, job: &CuratorJob, ctx: &CuratorContext) -> Result<CuratorRun> {
        let invocation = self.prepare_invocation(job, ctx)?;
        let mut command = Command::new(&self.config.binary);
        command
            .args(&invocation.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn `{}`", self.config.binary.display()))?;

        {
            let stdin = child
                .stdin
                .as_mut()
                .context("failed to open codex stdin for curator prompt")?;
            use std::io::Write;
            stdin.write_all(invocation.prompt.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            bail!("codex curator failed: {stderr}");
        }

        let raw = fs::read_to_string(&invocation.output_path).with_context(|| {
            format!(
                "codex curator did not produce output at {}",
                invocation.output_path.display()
            )
        })?;
        let parsed: CuratorRun = serde_json::from_str(&raw)
            .with_context(|| "failed to parse curator JSON output".to_string())?;

        let _ = fs::remove_file(&invocation.schema_path);
        let _ = fs::remove_file(&invocation.output_path);
        let _ = fs::remove_dir_all(&invocation.temp_dir);

        Ok(parsed)
    }
}

struct PreparedCodexInvocation {
    args: Vec<String>,
    prompt: String,
    schema_path: PathBuf,
    output_path: PathBuf,
    temp_dir: PathBuf,
}

fn curator_run_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["proposals", "diagnostics"],
        "properties": {
            "proposals": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["kind"],
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": [
                                "inferred_edge",
                                "structural_memory",
                                "risk_summary",
                                "validation_recipe"
                            ]
                        }
                    },
                    "additionalProperties": true
                }
            },
            "diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "required": ["code", "message"],
                    "properties": {
                        "code": { "type": "string" },
                        "message": { "type": "string" },
                        "data": {}
                    }
                }
            }
        }
    })
}

fn bounded_context(ctx: &CuratorContext, budget: &CuratorBudget) -> CuratorContext {
    let mut bounded = ctx.clone();
    if bounded.graph.nodes.len() > budget.max_context_nodes {
        bounded.graph.nodes.truncate(budget.max_context_nodes);
    }
    let max_edges = budget.max_context_nodes.saturating_mul(4).max(1);
    if bounded.graph.edges.len() > max_edges {
        bounded.graph.edges.truncate(max_edges);
    }
    if bounded.outcomes.len() > budget.max_outcomes {
        bounded.outcomes.truncate(budget.max_outcomes);
    }
    if bounded.memories.len() > budget.max_memories {
        bounded.memories.truncate(budget.max_memories);
    }
    if bounded.projections.co_change.len() > budget.max_context_nodes {
        bounded
            .projections
            .co_change
            .truncate(budget.max_context_nodes);
    }
    if bounded.projections.validation_checks.len() > budget.max_context_nodes {
        bounded
            .projections
            .validation_checks
            .truncate(budget.max_context_nodes);
    }
    bounded
}

fn render_toml_value(value: &TomlValue) -> Result<String> {
    match value {
        TomlValue::String(text) => Ok(TomlValue::String(text.clone()).to_string()),
        TomlValue::Integer(number) => Ok(number.to_string()),
        TomlValue::Float(number) => Ok(number.to_string()),
        TomlValue::Boolean(value) => Ok(value.to_string()),
        TomlValue::Datetime(value) => Ok(value.to_string()),
        TomlValue::Array(_) | TomlValue::Table(_) => {
            let rendered = toml::to_string(value)?.trim().to_string();
            if rendered.is_empty() {
                return Err(anyhow!("failed to render TOML override"));
            }
            Ok(rendered)
        }
    }
}

fn unique_temp_dir(prefix: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_ir::{EdgeKind, EdgeOrigin, Language, NodeKind, Span};

    fn sample_job() -> CuratorJob {
        CuratorJob {
            id: CuratorJobId("job:1".to_string()),
            trigger: CuratorTrigger::PostChange,
            task: Some(TaskId::new("task:1")),
            focus: vec![AnchorRef::Node(NodeId::new(
                "demo",
                "demo::alpha",
                NodeKind::Function,
            ))],
            budget: CuratorBudget::default(),
        }
    }

    fn sample_context() -> CuratorContext {
        CuratorContext {
            graph: CuratorGraphSlice {
                nodes: vec![Node {
                    id: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                    name: "alpha".into(),
                    kind: NodeKind::Function,
                    file: prism_ir::FileId(1),
                    span: Span::line(1),
                    language: Language::Rust,
                }],
                edges: vec![Edge {
                    kind: EdgeKind::Calls,
                    source: NodeId::new("demo", "demo::alpha", NodeKind::Function),
                    target: NodeId::new("demo", "demo::beta", NodeKind::Function),
                    origin: EdgeOrigin::Static,
                    confidence: 1.0,
                }],
            },
            ..CuratorContext::default()
        }
    }

    #[test]
    fn invocation_includes_typed_codex_options() {
        let mut config = CodexCliCuratorConfig::codex("codex", "/tmp/demo");
        config.model = Some("gpt-5.4".to_string());
        config.profile = Some("curator".to_string());
        config.sandbox = Some(CodexSandboxMode::WorkspaceWrite);
        config.approval_policy = Some(CodexApprovalPolicy::Never);
        config.reasoning_effort = Some(CodexReasoningEffort::High);
        config.execution_mode = CodexExecutionMode::FullAuto;
        config.add_dirs.push("/tmp/extra".into());
        config.skip_git_repo_check = true;
        config.ephemeral = true;
        config.enable_features.push("foo".to_string());
        config.disable_features.push("bar".to_string());

        let curator = CodexCliCurator::new(config);
        let invocation = curator
            .prepare_invocation(&sample_job(), &sample_context())
            .expect("invocation should prepare");

        assert_eq!(invocation.args[0], "exec");
        assert!(invocation
            .args
            .windows(2)
            .any(|pair| pair == ["-m", "gpt-5.4"]));
        assert!(invocation
            .args
            .windows(2)
            .any(|pair| pair == ["-p", "curator"]));
        assert!(invocation
            .args
            .windows(2)
            .any(|pair| pair == ["-s", "workspace-write"]));
        assert!(invocation.args.iter().any(|arg| arg == "--full-auto"));
        assert!(invocation
            .args
            .iter()
            .any(|arg| arg == "approval_policy=\"never\""));
        assert!(invocation
            .args
            .iter()
            .any(|arg| arg == "reasoning_effort=\"high\""));
        assert!(invocation
            .args
            .iter()
            .any(|arg| arg == "--skip-git-repo-check"));
        assert!(invocation.args.iter().any(|arg| arg == "--ephemeral"));
    }

    #[test]
    fn bounded_context_respects_budget() {
        let mut context = sample_context();
        context.graph.nodes = (0..200)
            .map(|index| Node {
                id: NodeId::new("demo", format!("demo::node{index}"), NodeKind::Function),
                name: format!("node{index}").into(),
                kind: NodeKind::Function,
                file: prism_ir::FileId(1),
                span: Span::line(1),
                language: Language::Rust,
            })
            .collect();

        let bounded = bounded_context(
            &context,
            &CuratorBudget {
                max_context_nodes: 8,
                ..CuratorBudget::default()
            },
        );
        assert_eq!(bounded.graph.nodes.len(), 8);
        assert!(bounded.graph.edges.len() <= 32);
    }

    #[cfg(unix)]
    #[test]
    fn codex_backend_can_parse_structured_output() {
        use std::os::unix::fs::PermissionsExt;

        let script_dir = unique_temp_dir("prism-curator-test").expect("temp dir");
        let script_path = script_dir.join("fake-codex.sh");
        fs::write(
            &script_path,
            r#"#!/bin/sh
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o|--output-last-message)
      out="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
cat >/dev/null
printf '%s' '{"proposals":[{"kind":"risk_summary","anchors":[],"summary":"watch beta","severity":"medium","evidence_events":[]}],"diagnostics":[]}' > "$out"
"#,
        )
        .expect("script should write");
        let mut permissions = fs::metadata(&script_path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("permissions should set");

        let curator = CodexCliCurator::new(CodexCliCuratorConfig::codex(
            script_path.clone(),
            "/tmp/demo",
        ));
        let run = curator
            .run(&sample_job(), &sample_context())
            .expect("fake codex should run");

        assert_eq!(run.proposals.len(), 1);
        match &run.proposals[0] {
            CuratorProposal::RiskSummary(summary) => {
                assert_eq!(summary.summary, "watch beta");
                assert_eq!(summary.severity, "medium");
            }
            other => panic!("unexpected proposal: {other:?}"),
        }

        let _ = fs::remove_file(&script_path);
        let _ = fs::remove_dir_all(&script_dir);
    }
}
