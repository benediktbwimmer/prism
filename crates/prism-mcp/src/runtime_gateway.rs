use std::path::PathBuf;

use anyhow::Result;
use prism_core::CoordinationAuthorityStoreProvider;

use crate::peer_runtime_router::{
    execute_remote_prism_query_with_provider, RemotePrismQueryResult,
};
use crate::QueryLanguage;

#[derive(Clone)]
pub(crate) struct WorkspaceRuntimeGateway {
    root: PathBuf,
    authority_store_provider: CoordinationAuthorityStoreProvider,
}

impl WorkspaceRuntimeGateway {
    pub(crate) fn new(
        root: PathBuf,
        authority_store_provider: CoordinationAuthorityStoreProvider,
    ) -> Self {
        Self {
            root,
            authority_store_provider,
        }
    }

    pub(crate) fn execute_remote_prism_query(
        &self,
        runtime_id: &str,
        code: &str,
        language: QueryLanguage,
    ) -> Result<RemotePrismQueryResult> {
        execute_remote_prism_query_with_provider(
            &self.root,
            Some(&self.authority_store_provider),
            runtime_id,
            code,
            language,
        )
    }
}
