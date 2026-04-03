use std::path::Path;

use anyhow::Result;

use crate::prism_doc::{sync_repo_prism_doc, PrismDocSyncResult};
use crate::tracked_snapshot::{
    load_concept_snapshots, load_contract_snapshots, load_relation_snapshots,
    regenerate_tracked_snapshot_derived_artifacts,
};

pub fn regenerate_repo_snapshot_derived_artifacts(root: &Path) -> Result<PrismDocSyncResult> {
    regenerate_tracked_snapshot_derived_artifacts(root)?;
    let concepts = load_concept_snapshots(root)?;
    let relations = load_relation_snapshots(root)?;
    let contracts = load_contract_snapshots(root)?;
    sync_repo_prism_doc(root, &concepts, &relations, &contracts)
}
