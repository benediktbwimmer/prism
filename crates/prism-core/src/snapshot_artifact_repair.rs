use std::path::Path;

use anyhow::Result;

use crate::tracked_snapshot::regenerate_tracked_snapshot_derived_artifacts;

pub fn regenerate_repo_snapshot_derived_artifacts(root: &Path) -> Result<()> {
    regenerate_tracked_snapshot_derived_artifacts(root)
}
