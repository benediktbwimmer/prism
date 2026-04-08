use std::path::PathBuf;

use serde_yaml::Mapping;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecRootSource {
    Default,
    RepoConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecRootResolution {
    pub configured_root: PathBuf,
    pub absolute_root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub source: SpecRootSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSpecSource {
    pub repo_relative_path: PathBuf,
    pub absolute_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecDeclaredStatus {
    Draft,
    InProgress,
    Blocked,
    Completed,
    Superseded,
    Abandoned,
}

impl SpecDeclaredStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "in_progress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "completed" => Some(Self::Completed),
            "superseded" => Some(Self::Superseded),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Superseded => "superseded",
            Self::Abandoned => "abandoned",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSpecDocument {
    pub source: DiscoveredSpecSource,
    pub frontmatter: Mapping,
    pub body: String,
    pub spec_id: String,
    pub title: String,
    pub status: SpecDeclaredStatus,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecParseDiagnosticKind {
    MissingFrontmatter,
    MissingClosingFrontmatter,
    InvalidFrontmatterYaml,
    MissingRequiredField,
    InvalidFieldType,
    InvalidStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecParseDiagnostic {
    pub source_path: PathBuf,
    pub kind: SpecParseDiagnosticKind,
    pub field: Option<String>,
    pub message: String,
}
