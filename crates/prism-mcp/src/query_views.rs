use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::{
    query_view_after_edit::after_edit_view, query_view_command_memory::command_memory_view,
    query_view_impact::impact_view, query_view_playbook::repo_playbook_view,
    query_view_validation_plan::validation_plan_view, PrismMcpFeatures, QueryExecution, QueryHost,
    QueryViewCapabilityView, QueryViewFeatureFlag,
};

#[derive(Debug, Clone, Copy)]
enum QueryViewStability {
    Experimental,
}

impl QueryViewStability {
    fn label(self) -> &'static str {
        match self {
            QueryViewStability::Experimental => "experimental",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct QueryViewDefinition {
    name: &'static str,
    feature_flag: QueryViewFeatureFlag,
    stability: QueryViewStability,
    owner: &'static str,
    description: &'static str,
    advertise_when_disabled: bool,
}

impl QueryViewDefinition {
    fn enabled(self, features: &PrismMcpFeatures) -> bool {
        features.query_view_enabled(self.feature_flag)
    }

    fn capability(self, features: &PrismMcpFeatures) -> QueryViewCapabilityView {
        QueryViewCapabilityView {
            name: self.name.to_string(),
            enabled: self.enabled(features),
            feature_flag: self.feature_flag.key().to_string(),
            stability: self.stability.label().to_string(),
            owner: self.owner.to_string(),
            description: self.description.to_string(),
        }
    }

    fn invoke(self, execution: &QueryExecution, _input: Value) -> Result<Value> {
        match self.name {
            "repoPlaybook" => repo_playbook_view(execution),
            "validationPlan" => validation_plan_view(execution, _input),
            "impact" => impact_view(execution, _input),
            "afterEdit" => after_edit_view(execution, _input),
            "commandMemory" => command_memory_view(execution, _input),
            #[cfg(test)]
            "testEcho" => Ok(test_echo_view(_input)),
            _ => not_implemented_view(self, execution),
        }
    }
}

#[cfg(test)]
fn test_echo_view(input: Value) -> Value {
    serde_json::json!({ "echo": input })
}

fn not_implemented_view(view: QueryViewDefinition, _execution: &QueryExecution) -> Result<Value> {
    Err(anyhow!(
        "query view `{}` is registered behind feature flag `{}` but is not implemented yet",
        view.name,
        view.feature_flag.key()
    ))
}

fn query_view_definitions() -> Vec<QueryViewDefinition> {
    let views = vec![
        QueryViewDefinition {
            name: "repoPlaybook",
            feature_flag: QueryViewFeatureFlag::RepoPlaybook,
            stability: QueryViewStability::Experimental,
            owner: "prism-mcp",
            description: "Summarize repo-specific build, test, lint, format, and workflow guidance.",
            advertise_when_disabled: true,
        },
        QueryViewDefinition {
            name: "validationPlan",
            feature_flag: QueryViewFeatureFlag::ValidationPlan,
            stability: QueryViewStability::Experimental,
            owner: "prism-mcp",
            description: "Map a task, path set, or target to the smallest likely validations with provenance.",
            advertise_when_disabled: true,
        },
        QueryViewDefinition {
            name: "impact",
            feature_flag: QueryViewFeatureFlag::Impact,
            stability: QueryViewStability::Experimental,
            owner: "prism-mcp",
            description: "Summarize likely downstream impact, risk hints, and recommended checks.",
            advertise_when_disabled: true,
        },
        QueryViewDefinition {
            name: "afterEdit",
            feature_flag: QueryViewFeatureFlag::AfterEdit,
            stability: QueryViewStability::Experimental,
            owner: "prism-mcp",
            description: "Turn recent edits into the next high-value reads, tests, and docs to inspect.",
            advertise_when_disabled: true,
        },
        QueryViewDefinition {
            name: "commandMemory",
            feature_flag: QueryViewFeatureFlag::CommandMemory,
            stability: QueryViewStability::Experimental,
            owner: "prism-mcp",
            description: "Recommend repo-specific commands from explicit observed evidence and workflow signals.",
            advertise_when_disabled: true,
        },
    ];
    #[cfg(not(test))]
    {
        return views;
    }
    #[cfg(test)]
    {
        let mut views = views;
        views.push(QueryViewDefinition {
            name: "testEcho",
            feature_flag: QueryViewFeatureFlag::TestEcho,
            stability: QueryViewStability::Experimental,
            owner: "tests",
            description: "Round-trip a test payload through the dynamic query-view registry.",
            advertise_when_disabled: false,
        });
        views
    }
}

pub(crate) fn known_query_view_names() -> Vec<&'static str> {
    query_view_definitions()
        .into_iter()
        .map(|view| view.name)
        .collect()
}

impl QueryHost {
    pub(crate) fn query_view_capabilities(&self) -> Vec<QueryViewCapabilityView> {
        if !self.features.cognition_layer_enabled() {
            return Vec::new();
        }
        query_view_definitions()
            .into_iter()
            .filter(|view| view.advertise_when_disabled || view.enabled(&self.features))
            .map(|view| view.capability(&self.features))
            .collect()
    }

    pub(crate) fn enabled_query_view_capabilities(&self) -> Vec<QueryViewCapabilityView> {
        if !self.features.cognition_layer_enabled() {
            return Vec::new();
        }
        query_view_definitions()
            .into_iter()
            .filter(|view| view.enabled(&self.features))
            .map(|view| view.capability(&self.features))
            .collect()
    }
}

impl QueryExecution {
    pub(crate) fn dispatch_query_view(&self, name: &str, input: Value) -> Result<Value> {
        let Some(view) = query_view_definitions()
            .into_iter()
            .find(|candidate| candidate.name == name)
        else {
            return Err(anyhow!("unknown query view `{name}`"));
        };
        if !self.query_view_enabled(view.feature_flag) {
            return Err(anyhow!(
                "query view `{name}` is disabled; enable feature flag `{}` to expose it at runtime",
                view.feature_flag.key()
            ));
        }
        self.query_run().set_view_name(name);
        view.invoke(self, input)
    }
}
