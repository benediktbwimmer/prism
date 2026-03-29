use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use prism_ir::{AgentId, TaskId};
use prism_query::QueryLimits;

use crate::SessionState;

const SESSION_SEED_FILE_NAME: &str = "prism-mcp-session-seed.json";
const SESSION_SEED_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PersistedSessionSeed {
    version: u32,
    current_task: Option<PersistedSessionTaskSeed>,
    current_agent: Option<String>,
    limits: PersistedQueryLimits,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedSessionTaskSeed {
    task_id: String,
    description: Option<String>,
    tags: Vec<String>,
    coordination_task_id: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct PersistedQueryLimits {
    max_result_nodes: usize,
    max_call_graph_depth: usize,
    max_output_json_bytes: usize,
}

impl PersistedSessionSeed {
    fn capture(session: &SessionState) -> Self {
        Self {
            version: SESSION_SEED_VERSION,
            current_task: session
                .current_task_state()
                .map(|task| PersistedSessionTaskSeed {
                    task_id: task.id.0.to_string(),
                    description: task.description,
                    tags: task.tags,
                    coordination_task_id: task.coordination_task_id,
                }),
            current_agent: session.current_agent().map(|agent| agent.0.to_string()),
            limits: PersistedQueryLimits::from(session.limits()),
        }
    }
}

impl From<QueryLimits> for PersistedQueryLimits {
    fn from(value: QueryLimits) -> Self {
        Self {
            max_result_nodes: value.max_result_nodes,
            max_call_graph_depth: value.max_call_graph_depth,
            max_output_json_bytes: value.max_output_json_bytes,
        }
    }
}

impl From<PersistedQueryLimits> for QueryLimits {
    fn from(value: PersistedQueryLimits) -> Self {
        Self {
            max_result_nodes: value.max_result_nodes,
            max_call_graph_depth: value.max_call_graph_depth,
            max_output_json_bytes: value.max_output_json_bytes,
        }
    }
}

pub(crate) fn load_session_seed(root: &Path) -> Result<Option<PersistedSessionSeed>> {
    let path = session_seed_path(root);
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let seed: PersistedSessionSeed = serde_json::from_str(&contents)
                .with_context(|| format!("failed to parse session seed at {}", path.display()))?;
            Ok((seed.version == SESSION_SEED_VERSION).then_some(seed))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read session seed at {}", path.display()))
        }
    }
}

pub(crate) fn persist_session_seed(root: &Path, session: &SessionState) -> Result<()> {
    let path = session_seed_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create session seed directory {}",
                parent.display()
            )
        })?;
    }
    let bytes = serde_json::to_vec_pretty(&PersistedSessionSeed::capture(session))
        .context("failed to serialize session seed")?;
    fs::write(&path, bytes)
        .with_context(|| format!("failed to write session seed to {}", path.display()))
}

pub(crate) fn restore_session_seed(session: &SessionState, seed: &PersistedSessionSeed) {
    session.set_limits(seed.limits.into());
    if let Some(task) = &seed.current_task {
        session.set_current_task(
            TaskId::new(task.task_id.clone()),
            task.description.clone(),
            task.tags.clone(),
            task.coordination_task_id.clone(),
        );
    }
    if let Some(agent) = &seed.current_agent {
        session.set_current_agent(AgentId::new(agent.clone()));
    }
}

fn session_seed_path(root: &Path) -> PathBuf {
    root.join(".prism").join(SESSION_SEED_FILE_NAME)
}
