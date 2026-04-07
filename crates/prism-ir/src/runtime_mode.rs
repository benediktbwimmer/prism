use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrismRuntimeLayer {
    Coordination,
    KnowledgeStorage,
    Cognition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrismLayerSet {
    pub coordination: bool,
    pub knowledge_storage: bool,
    pub cognition: bool,
}

impl PrismLayerSet {
    pub const fn new(coordination: bool, knowledge_storage: bool, cognition: bool) -> Self {
        Self {
            coordination,
            knowledge_storage,
            cognition,
        }
    }

    pub const fn full() -> Self {
        Self::new(true, true, true)
    }

    pub const fn coordination_only() -> Self {
        Self::new(true, false, false)
    }

    pub const fn knowledge_storage() -> Self {
        Self::new(true, true, false)
    }

    pub const fn core_legacy() -> Self {
        Self::new(false, true, true)
    }

    pub const fn has(self, layer: PrismRuntimeLayer) -> bool {
        match layer {
            PrismRuntimeLayer::Coordination => self.coordination,
            PrismRuntimeLayer::KnowledgeStorage => self.knowledge_storage,
            PrismRuntimeLayer::Cognition => self.cognition,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrismRuntimeMode {
    #[default]
    Full,
    CoordinationOnly,
    KnowledgeStorage,
    /// Migration-only compatibility mode for legacy non-coordination runtimes.
    CoreLegacy,
}

impl PrismRuntimeMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::CoordinationOnly => "coordination_only",
            Self::KnowledgeStorage => "knowledge_storage",
            Self::CoreLegacy => "core_legacy",
        }
    }

    pub const fn layers(self) -> PrismLayerSet {
        match self {
            Self::Full => PrismLayerSet::full(),
            Self::CoordinationOnly => PrismLayerSet::coordination_only(),
            Self::KnowledgeStorage => PrismLayerSet::knowledge_storage(),
            Self::CoreLegacy => PrismLayerSet::core_legacy(),
        }
    }

    pub const fn from_capabilities(capabilities: PrismRuntimeCapabilities) -> Option<Self> {
        if capabilities.coordination && capabilities.knowledge_storage && capabilities.cognition {
            Some(Self::Full)
        } else if capabilities.coordination
            && !capabilities.knowledge_storage
            && !capabilities.cognition
        {
            Some(Self::CoordinationOnly)
        } else if capabilities.coordination
            && capabilities.knowledge_storage
            && !capabilities.cognition
        {
            Some(Self::KnowledgeStorage)
        } else if !capabilities.coordination
            && capabilities.knowledge_storage
            && capabilities.cognition
        {
            Some(Self::CoreLegacy)
        } else {
            None
        }
    }

    pub const fn capabilities(self) -> PrismRuntimeCapabilities {
        PrismRuntimeCapabilities::from_layers(self.layers())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrismRuntimeCapabilities {
    pub coordination: bool,
    pub knowledge_storage: bool,
    pub cognition: bool,
}

impl PrismRuntimeCapabilities {
    pub const fn from_layers(layers: PrismLayerSet) -> Self {
        Self {
            coordination: layers.coordination,
            knowledge_storage: layers.knowledge_storage,
            cognition: layers.cognition,
        }
    }

    pub const fn coordination_enabled(self) -> bool {
        self.coordination
    }

    pub const fn knowledge_storage_enabled(self) -> bool {
        self.knowledge_storage
    }

    pub const fn cognition_enabled(self) -> bool {
        self.cognition
    }

    pub const fn graph_backed_resolution_enabled(self) -> bool {
        self.cognition
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_runtime_modes_map_to_expected_layers() {
        assert_eq!(PrismRuntimeMode::Full.layers(), PrismLayerSet::full());
        assert_eq!(
            PrismRuntimeMode::CoordinationOnly.layers(),
            PrismLayerSet::coordination_only()
        );
        assert_eq!(
            PrismRuntimeMode::KnowledgeStorage.layers(),
            PrismLayerSet::knowledge_storage()
        );
        assert_eq!(
            PrismRuntimeMode::CoreLegacy.layers(),
            PrismLayerSet::core_legacy()
        );
    }

    #[test]
    fn supported_runtime_modes_map_to_expected_capabilities() {
        let coordination_only = PrismRuntimeMode::CoordinationOnly.capabilities();
        assert!(coordination_only.coordination_enabled());
        assert!(!coordination_only.knowledge_storage_enabled());
        assert!(!coordination_only.graph_backed_resolution_enabled());

        let knowledge_storage = PrismRuntimeMode::KnowledgeStorage.capabilities();
        assert!(knowledge_storage.coordination_enabled());
        assert!(knowledge_storage.knowledge_storage_enabled());
        assert!(!knowledge_storage.graph_backed_resolution_enabled());
    }

    #[test]
    fn supported_runtime_capabilities_map_back_to_modes() {
        assert_eq!(
            PrismRuntimeMode::from_capabilities(PrismRuntimeMode::Full.capabilities()),
            Some(PrismRuntimeMode::Full)
        );
        assert_eq!(
            PrismRuntimeMode::from_capabilities(PrismRuntimeMode::CoordinationOnly.capabilities()),
            Some(PrismRuntimeMode::CoordinationOnly)
        );
        assert_eq!(
            PrismRuntimeMode::from_capabilities(PrismRuntimeMode::KnowledgeStorage.capabilities()),
            Some(PrismRuntimeMode::KnowledgeStorage)
        );
        assert_eq!(
            PrismRuntimeMode::from_capabilities(PrismRuntimeMode::CoreLegacy.capabilities()),
            Some(PrismRuntimeMode::CoreLegacy)
        );
        assert_eq!(
            PrismRuntimeMode::from_capabilities(PrismRuntimeCapabilities {
                coordination: false,
                knowledge_storage: false,
                cognition: false,
            }),
            None
        );
    }
}
