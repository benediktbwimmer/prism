use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use toml::Value as TomlValue;

use crate::support::{bounded_context, curator_run_schema, render_toml_value, unique_temp_dir};
use crate::types::{CuratorBackend, CuratorContext, CuratorJob, CuratorRun};

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
- semantic_memory\n\
- risk_summary\n\
- validation_recipe\n\
- concept_candidate\n\
\n\
JSON payload:\n\
{payload}\n"
        ))
    }

    pub(crate) fn prepare_invocation(
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

pub(crate) struct PreparedCodexInvocation {
    pub(crate) args: Vec<String>,
    pub(crate) prompt: String,
    pub(crate) schema_path: PathBuf,
    pub(crate) output_path: PathBuf,
    pub(crate) temp_dir: PathBuf,
}
