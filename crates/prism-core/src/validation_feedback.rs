use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use prism_ir::{new_prefixed_id, AnchorRef, EventActor, EventExecutionContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::util::{current_timestamp, validation_feedback_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationFeedbackCategory {
    Structural,
    Lineage,
    Memory,
    Projection,
    Coordination,
    Freshness,
    Other,
}

impl fmt::Display for ValidationFeedbackCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Structural => "structural",
            Self::Lineage => "lineage",
            Self::Memory => "memory",
            Self::Projection => "projection",
            Self::Coordination => "coordination",
            Self::Freshness => "freshness",
            Self::Other => "other",
        };
        f.write_str(value)
    }
}

impl FromStr for ValidationFeedbackCategory {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "structural" | "structure" => Ok(Self::Structural),
            "lineage" => Ok(Self::Lineage),
            "memory" => Ok(Self::Memory),
            "projection" => Ok(Self::Projection),
            "coordination" => Ok(Self::Coordination),
            "freshness" | "stale" => Ok(Self::Freshness),
            "other" => Ok(Self::Other),
            other => Err(format!("unknown validation feedback category `{other}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationFeedbackVerdict {
    Wrong,
    Stale,
    Noisy,
    Helpful,
    Mixed,
}

impl fmt::Display for ValidationFeedbackVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Wrong => "wrong",
            Self::Stale => "stale",
            Self::Noisy => "noisy",
            Self::Helpful => "helpful",
            Self::Mixed => "mixed",
        };
        f.write_str(value)
    }
}

impl FromStr for ValidationFeedbackVerdict {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "wrong" => Ok(Self::Wrong),
            "stale" => Ok(Self::Stale),
            "noisy" | "noise" => Ok(Self::Noisy),
            "helpful" | "win" => Ok(Self::Helpful),
            "mixed" => Ok(Self::Mixed),
            other => Err(format!("unknown validation feedback verdict `{other}`")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationFeedbackEntry {
    pub id: String,
    pub recorded_at: u64,
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<EventActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_context: Option<EventExecutionContext>,
    pub context: String,
    pub anchors: Vec<AnchorRef>,
    pub prism_said: String,
    pub actually_true: String,
    pub category: ValidationFeedbackCategory,
    pub verdict: ValidationFeedbackVerdict,
    pub corrected_manually: bool,
    pub correction: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationFeedbackRecord {
    pub task_id: Option<String>,
    pub actor: Option<EventActor>,
    pub execution_context: Option<EventExecutionContext>,
    pub context: String,
    pub anchors: Vec<AnchorRef>,
    pub prism_said: String,
    pub actually_true: String,
    pub category: ValidationFeedbackCategory,
    pub verdict: ValidationFeedbackVerdict,
    pub corrected_manually: bool,
    pub correction: Option<String>,
    pub metadata: Value,
}

impl ValidationFeedbackRecord {
    fn into_entry(self) -> ValidationFeedbackEntry {
        ValidationFeedbackEntry {
            id: next_feedback_id(),
            recorded_at: current_timestamp(),
            task_id: self.task_id,
            actor: self.actor,
            execution_context: self.execution_context,
            context: self.context,
            anchors: self.anchors,
            prism_said: self.prism_said,
            actually_true: self.actually_true,
            category: self.category,
            verdict: self.verdict,
            corrected_manually: self.corrected_manually,
            correction: self.correction,
            metadata: self.metadata,
        }
    }
}

pub(crate) fn append_validation_feedback(
    root: &Path,
    record: ValidationFeedbackRecord,
) -> Result<ValidationFeedbackEntry> {
    let path = validation_feedback_path(root)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let entry = record.into_entry();
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, &entry)?;
    file.write_all(b"\n")?;
    Ok(entry)
}

pub(crate) fn load_validation_feedback(root: &Path) -> Result<Vec<ValidationFeedbackEntry>> {
    let path = validation_feedback_path(root)?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str::<ValidationFeedbackEntry>(&line).with_context(|| {
            format!(
                "failed to parse validation feedback entry on line {} in {}",
                index + 1,
                path.display()
            )
        })?;
        entries.push(entry);
    }
    Ok(entries)
}

fn next_feedback_id() -> String {
    new_prefixed_id("feedback").to_string()
}
