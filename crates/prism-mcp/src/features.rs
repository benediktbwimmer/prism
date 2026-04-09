use clap::ValueEnum;
use prism_core::{PrismRuntimeCapabilities, PrismRuntimeMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum CoordinationFeatureFlag {
    Workflow,
    Claims,
    Artifacts,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum QueryViewFeatureFlag {
    RepoPlaybook,
    ValidationPlan,
    Impact,
    AfterEdit,
    CommandMemory,
    All,
    #[cfg(test)]
    TestEcho,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoordinationFeatureSet {
    pub(crate) workflow: bool,
    pub(crate) claims: bool,
    pub(crate) artifacts: bool,
}

impl CoordinationFeatureSet {
    pub(crate) fn full() -> Self {
        Self {
            workflow: true,
            claims: true,
            artifacts: true,
        }
    }

    pub(crate) fn simple() -> Self {
        Self {
            workflow: false,
            claims: false,
            artifacts: false,
        }
    }

    pub(crate) fn apply(&mut self, flag: CoordinationFeatureFlag, enabled: bool) {
        match flag {
            CoordinationFeatureFlag::Workflow => self.workflow = enabled,
            CoordinationFeatureFlag::Claims => self.claims = enabled,
            CoordinationFeatureFlag::Artifacts => self.artifacts = enabled,
            CoordinationFeatureFlag::All => {
                self.workflow = enabled;
                self.claims = enabled;
                self.artifacts = enabled;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct QueryViewFeatureSet {
    pub(crate) repo_playbook: bool,
    pub(crate) validation_plan: bool,
    pub(crate) impact: bool,
    pub(crate) after_edit: bool,
    pub(crate) command_memory: bool,
    #[cfg(test)]
    pub(crate) test_echo: bool,
}

impl QueryViewFeatureSet {
    pub(crate) fn full() -> Self {
        Self {
            repo_playbook: true,
            validation_plan: true,
            impact: true,
            after_edit: true,
            command_memory: true,
            #[cfg(test)]
            test_echo: false,
        }
    }

    pub(crate) fn apply(&mut self, flag: QueryViewFeatureFlag, enabled: bool) {
        match flag {
            QueryViewFeatureFlag::RepoPlaybook => self.repo_playbook = enabled,
            QueryViewFeatureFlag::ValidationPlan => self.validation_plan = enabled,
            QueryViewFeatureFlag::Impact => self.impact = enabled,
            QueryViewFeatureFlag::AfterEdit => self.after_edit = enabled,
            QueryViewFeatureFlag::CommandMemory => self.command_memory = enabled,
            QueryViewFeatureFlag::All => {
                self.repo_playbook = enabled;
                self.validation_plan = enabled;
                self.impact = enabled;
                self.after_edit = enabled;
                self.command_memory = enabled;
                #[cfg(test)]
                {
                    self.test_echo = enabled;
                }
            }
            #[cfg(test)]
            QueryViewFeatureFlag::TestEcho => self.test_echo = enabled,
        }
    }

    pub(crate) fn enabled(&self, flag: QueryViewFeatureFlag) -> bool {
        match flag {
            QueryViewFeatureFlag::RepoPlaybook => self.repo_playbook,
            QueryViewFeatureFlag::ValidationPlan => self.validation_plan,
            QueryViewFeatureFlag::Impact => self.impact,
            QueryViewFeatureFlag::AfterEdit => self.after_edit,
            QueryViewFeatureFlag::CommandMemory => self.command_memory,
            QueryViewFeatureFlag::All => {
                self.repo_playbook
                    && self.validation_plan
                    && self.impact
                    && self.after_edit
                    && self.command_memory
                    && {
                        #[cfg(test)]
                        {
                            self.test_echo
                        }
                        #[cfg(not(test))]
                        {
                            true
                        }
                    }
            }
            #[cfg(test)]
            QueryViewFeatureFlag::TestEcho => self.test_echo,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrismMcpFeatures {
    pub(crate) runtime_mode: PrismRuntimeMode,
    pub(crate) coordination: CoordinationFeatureSet,
    pub(crate) query_views: QueryViewFeatureSet,
    pub(crate) ui: bool,
    pub(crate) internal_developer: bool,
    pub(crate) runtime_diagnostics_auto_refresh: bool,
}

impl Default for PrismMcpFeatures {
    fn default() -> Self {
        Self::full()
    }
}

impl PrismMcpFeatures {
    pub fn full() -> Self {
        Self {
            runtime_mode: PrismRuntimeMode::Full,
            coordination: CoordinationFeatureSet::full(),
            query_views: QueryViewFeatureSet::full(),
            ui: false,
            internal_developer: false,
            runtime_diagnostics_auto_refresh: true,
        }
    }

    pub fn simple() -> Self {
        Self {
            runtime_mode: PrismRuntimeMode::Full,
            coordination: CoordinationFeatureSet::simple(),
            query_views: QueryViewFeatureSet::default(),
            ui: false,
            internal_developer: false,
            runtime_diagnostics_auto_refresh: true,
        }
    }

    pub fn with_ui(mut self, enabled: bool) -> Self {
        self.ui = enabled;
        self
    }

    pub fn with_internal_developer(mut self, enabled: bool) -> Self {
        self.internal_developer = enabled;
        self
    }

    pub fn with_runtime_diagnostics_auto_refresh(mut self, enabled: bool) -> Self {
        self.runtime_diagnostics_auto_refresh = enabled;
        self
    }

    pub fn with_runtime_mode(mut self, runtime_mode: PrismRuntimeMode) -> Self {
        self.runtime_mode = runtime_mode;
        if !self.runtime_mode.capabilities().cognition_enabled() {
            self.query_views = QueryViewFeatureSet::default();
        }
        self
    }

    pub fn with_query_view(mut self, flag: QueryViewFeatureFlag, enabled: bool) -> Self {
        self.query_views.apply(flag, enabled);
        self
    }

    pub(crate) fn mode_label(&self) -> &'static str {
        match self.coordination {
            CoordinationFeatureSet {
                workflow: true,
                claims: true,
                artifacts: true,
            } => "full",
            CoordinationFeatureSet {
                workflow: false,
                claims: false,
                artifacts: false,
            } => "simple",
            _ => "custom",
        }
    }

    pub(crate) fn runtime_mode(&self) -> PrismRuntimeMode {
        self.runtime_mode
    }

    pub(crate) fn runtime_mode_label(&self) -> &'static str {
        self.runtime_mode.label()
    }

    pub(crate) fn runtime_capabilities(&self) -> PrismRuntimeCapabilities {
        self.runtime_mode.capabilities()
    }

    pub(crate) fn is_tool_enabled(&self, name: &str) -> bool {
        if self.runtime_mode == PrismRuntimeMode::CoordinationOnly {
            return matches!(name, "prism_query" | "prism_task_brief" | "prism_mutate");
        }
        match name {
            "prism_coordination" => self.coordination.workflow,
            "prism_claim" => self.coordination.claims,
            "prism_artifact" => self.coordination.artifacts,
            _ => true,
        }
    }

    pub(crate) fn prism_mutate_action_visible(&self, action: &str) -> bool {
        if self.runtime_mode != PrismRuntimeMode::CoordinationOnly {
            return true;
        }
        match action {
            "declare_work" => true,
            "coordination" => self.coordination.workflow,
            "heartbeat_lease" | "claim" => self.coordination.claims,
            "artifact" => self.coordination.artifacts,
            _ => false,
        }
    }

    pub(crate) fn prism_mutate_action_enabled(&self, action: &str) -> bool {
        if !self.prism_mutate_action_visible(action) {
            return false;
        }
        match action {
            "coordination" => self.coordination.workflow,
            "heartbeat_lease" | "claim" => self.coordination.claims,
            "artifact" => self.coordination.artifacts,
            _ => true,
        }
    }

    pub(crate) fn tool_example_resources_visible(&self) -> bool {
        self.runtime_mode != PrismRuntimeMode::CoordinationOnly
    }

    pub(crate) fn resource_example_resources_visible(&self) -> bool {
        self.runtime_mode != PrismRuntimeMode::CoordinationOnly
    }

    fn coordination_only_query_method_enabled(&self, operation: &str) -> bool {
        match operation {
            "from" | "tools" | "tool" | "validateToolInput" | "diagnostics" => true,
            "plans" | "plan" | "planSummary" | "task" | "readyTasks" | "blockers"
            | "policyViolations"
                if self.coordination.workflow =>
            {
                true
            }
            "claims" | "conflicts" | "simulateClaim" if self.coordination.claims => true,
            "pendingReviews" | "artifacts" | "artifactRisk" | "taskEvidenceStatus"
            | "taskReviewStatus"
                if self.coordination.artifacts =>
            {
                true
            }
            "runtimeStatus" | "runtimeLogs" | "runtimeTimeline" | "mcpLog" | "slowMcpCalls"
            | "mcpTrace" | "mcpStats" | "queryLog" | "slowQueries" | "queryTrace"
            | "validationFeedback"
                if self.internal_developer =>
            {
                true
            }
            _ => false,
        }
    }

    pub(crate) fn disabled_query_group(&self, operation: &str) -> Option<&'static str> {
        if self.runtime_mode == PrismRuntimeMode::CoordinationOnly {
            return if self.coordination_only_query_method_enabled(operation) {
                None
            } else {
                Some("cognition")
            };
        }
        match operation {
            "runtimeStatus" | "runtimeLogs" | "runtimeTimeline" | "mcpLog" | "slowMcpCalls"
            | "mcpTrace" | "mcpStats" | "queryLog" | "slowQueries" | "queryTrace"
            | "validationFeedback"
                if !self.internal_developer =>
            {
                Some("internal_developer")
            }
            "plans"
            | "plan"
            | "planSummary"
            | "task"
            | "readyTasks"
            | "blockers"
            | "policyViolations"
            | "taskBlastRadius"
            | "taskValidationRecipe"
            | "taskRisk"
            | "taskIntent"
                if !self.coordination.workflow =>
            {
                Some("workflow")
            }
            "claims" | "conflicts" | "simulateClaim" if !self.coordination.claims => Some("claims"),
            "pendingReviews" | "artifacts" | "artifactRisk" | "taskEvidenceStatus"
            | "taskReviewStatus"
                if !self.coordination.artifacts =>
            {
                Some("artifacts")
            }
            _ => None,
        }
    }

    pub(crate) fn coordination_summary_lines(&self) -> Vec<String> {
        vec![
            format!("- runtime mode: `{}`", self.runtime_mode_label()),
            format!(
                "- coordination workflow: {}",
                enabled_label(self.coordination.workflow)
            ),
            format!(
                "- coordination claims: {}",
                enabled_label(self.coordination.claims)
            ),
            format!(
                "- coordination artifacts: {}",
                enabled_label(self.coordination.artifacts)
            ),
            format!(
                "- internal developer queries: {}",
                enabled_label(self.internal_developer)
            ),
        ]
    }

    pub(crate) fn coordination_layer_enabled(&self) -> bool {
        self.runtime_capabilities().coordination_enabled()
    }

    pub(crate) fn knowledge_storage_layer_enabled(&self) -> bool {
        self.runtime_capabilities().knowledge_storage_enabled()
    }

    pub(crate) fn cognition_layer_enabled(&self) -> bool {
        self.runtime_capabilities().cognition_enabled()
    }

    pub(crate) fn api_reference_includes_internal_developer(&self) -> bool {
        self.internal_developer
    }

    pub(crate) fn internal_developer_enabled(&self) -> bool {
        self.internal_developer
    }

    pub(crate) fn query_method_visible(&self, operation: &str) -> bool {
        !matches!(
            self.disabled_query_group(operation),
            Some("internal_developer") | Some("cognition")
        )
    }

    pub(crate) fn query_view_enabled(&self, flag: QueryViewFeatureFlag) -> bool {
        self.cognition_layer_enabled() && self.query_views.enabled(flag)
    }

    pub(crate) fn resource_kind_visible(&self, resource_kind: &str) -> bool {
        match resource_kind {
            "capabilities"
            | "session"
            | "protected-state"
            | "vocab"
            | "plans"
            | "plan"
            | "tool-schemas"
            | "capabilities-section"
            | "vocab-entry" => true,
            "tool-example" | "tool-shape" => self.tool_example_resources_visible(),
            "resource-example" | "resource-shape" => self.resource_example_resources_visible(),
            "contracts"
            | "schemas"
            | "self-description-audit"
            | "entrypoints"
            | "search"
            | "file"
            | "symbol"
            | "lineage"
            | "task"
            | "event"
            | "memory"
            | "edge" => self.cognition_layer_enabled(),
            _ => true,
        }
    }

    pub(crate) fn vocabulary_category_visible(&self, key: &str) -> bool {
        match key {
            "coordinationMutationKind"
            | "coordinationTaskStatus"
            | "planStatus"
            | "planScope"
            | "acceptanceEvidencePolicy" => self.coordination.workflow,
            "claimAction" | "capability" | "claimMode" => self.coordination.claims,
            "artifactAction" | "reviewVerdict" => self.coordination.artifacts,
            "prismLocateTaskIntent" => self.is_tool_enabled("prism_locate"),
            "prismOpenMode" => self.is_tool_enabled("prism_open"),
            "prismExpandKind" => self.is_tool_enabled("prism_expand"),
            "prismConceptLens" => self.is_tool_enabled("prism_concept"),
            _ => true,
        }
    }

    pub(crate) fn vocabulary_value_visible(&self, key: &str, value: &str) -> bool {
        match key {
            "prismMutateAction" => self.prism_mutate_action_visible(value),
            _ => self.vocabulary_category_visible(key),
        }
    }
}

impl QueryViewFeatureFlag {
    pub(crate) fn key(self) -> &'static str {
        match self {
            QueryViewFeatureFlag::RepoPlaybook => "repo_playbook",
            QueryViewFeatureFlag::ValidationPlan => "validation_plan",
            QueryViewFeatureFlag::Impact => "impact",
            QueryViewFeatureFlag::AfterEdit => "after_edit",
            QueryViewFeatureFlag::CommandMemory => "command_memory",
            QueryViewFeatureFlag::All => "all",
            #[cfg(test)]
            QueryViewFeatureFlag::TestEcho => "test_echo",
        }
    }
}

fn enabled_label(enabled: bool) -> &'static str {
    if enabled {
        "enabled"
    } else {
        "disabled"
    }
}
