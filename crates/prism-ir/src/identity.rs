use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
        )]
        pub struct $name(#[schemars(with = "String")] pub SmolStr);

        impl $name {
            pub fn new(value: impl Into<SmolStr>) -> Self {
                Self(value.into())
            }
        }
    };
}

string_id!(LineageId);
string_id!(EventId);
string_id!(TaskId);
string_id!(AgentId);
string_id!(SessionId);
string_id!(PrincipalAuthorityId);
string_id!(PrincipalId);
string_id!(CredentialId);
string_id!(PlanId);
string_id!(PlanNodeId);
string_id!(PlanEdgeId);
string_id!(CoordinationTaskId);
string_id!(ClaimId);
string_id!(ArtifactId);
string_id!(ReviewId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
pub struct WorkspaceRevision {
    pub graph_version: u64,
    #[schemars(with = "Option<String>")]
    pub git_commit: Option<SmolStr>,
}
