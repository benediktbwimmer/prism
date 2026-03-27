use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum CoordinationFeatureFlag {
    Workflow,
    Claims,
    Artifacts,
    All,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrismMcpFeatures {
    pub(crate) coordination: CoordinationFeatureSet,
    pub(crate) internal_developer: bool,
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
            internal_developer: false,
        }
    }

    pub fn simple() -> Self {
        Self {
            coordination: CoordinationFeatureSet::simple(),
            internal_developer: false,
        }
    }

    pub fn with_internal_developer(mut self, enabled: bool) -> Self {
        self.internal_developer = enabled;
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
            "runtimeStatus" | "runtimeLogs" | "runtimeTimeline" | "queryLog" | "slowQueries"
            | "queryTrace" | "validationFeedback"
                if !self.internal_developer =>
            {
                Some("internal_developer")
            }
            "plan"
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
}

fn enabled_label(enabled: bool) -> &'static str {
    if enabled {
        "enabled"
    } else {
        "disabled"
    }
}
