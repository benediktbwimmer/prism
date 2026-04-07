#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SharedRuntimeBackend {
    #[default]
    Disabled,
    Remote {
        uri: String,
    },
}

impl SharedRuntimeBackend {
    pub fn remote_uri(&self) -> Option<&str> {
        match self {
            SharedRuntimeBackend::Remote { uri } => Some(uri.as_str()),
            SharedRuntimeBackend::Disabled => None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self, SharedRuntimeBackend::Disabled)
    }
}
