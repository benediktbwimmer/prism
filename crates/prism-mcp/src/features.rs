use clap::ValueEnum;

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
            coordination: CoordinationFeatureSet::full(),
            query_views: QueryViewFeatureSet::full(),
            ui: false,
            internal_developer: false,
            runtime_diagnostics_auto_refresh: true,
        }
    }

    pub fn simple() -> Self {
        Self {
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

    pub(crate) fn is_tool_enabled(&self, name: &str) -> bool {
        match name {
            "prism_coordination" => self.coordination.workflow,
            "prism_claim" => self.coordination.claims,
            "prism_artifact" => self.coordination.artifacts,
            _ => true,
        }
    }

    pub(crate) fn disabled_query_group(&self, operation: &str) -> Option<&'static str> {
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
            | "planGraph"
            | "planProjectionAt"
            | "planProjectionDiff"
            | "planExecution"
            | "planReadyNodes"
            | "planNodeBlockers"
            | "planSummary"
            | "planNext"
            | "portfolioNext"
            | "coordinationTask"
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
            "pendingReviews" | "artifacts" | "artifactRisk" if !self.coordination.artifacts => {
                Some("artifacts")
            }
            _ => None,
        }
    }

    pub(crate) fn coordination_summary_lines(&self) -> Vec<String> {
        vec![
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
        self.coordination.workflow || self.coordination.claims || self.coordination.artifacts
    }

    pub(crate) fn api_reference_includes_internal_developer(&self) -> bool {
        self.internal_developer
    }

    pub(crate) fn query_method_visible(&self, operation: &str) -> bool {
        !matches!(
            self.disabled_query_group(operation),
            Some("internal_developer")
        )
    }

    pub(crate) fn query_view_enabled(&self, flag: QueryViewFeatureFlag) -> bool {
        self.query_views.enabled(flag)
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
