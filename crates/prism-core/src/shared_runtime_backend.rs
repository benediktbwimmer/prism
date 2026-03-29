use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SharedRuntimeBackend {
    #[default]
    Disabled,
    Sqlite { path: PathBuf },
    Remote { uri: String },
}

impl SharedRuntimeBackend {
    pub fn sqlite_path(&self) -> Option<&Path> {
        match self {
            SharedRuntimeBackend::Sqlite { path } => Some(path.as_path()),
            SharedRuntimeBackend::Disabled | SharedRuntimeBackend::Remote { .. } => None,
        }
    }

    pub fn remote_uri(&self) -> Option<&str> {
        match self {
            SharedRuntimeBackend::Remote { uri } => Some(uri.as_str()),
            SharedRuntimeBackend::Disabled | SharedRuntimeBackend::Sqlite { .. } => None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self, SharedRuntimeBackend::Disabled)
    }
}
