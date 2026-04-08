use std::path::PathBuf;

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
