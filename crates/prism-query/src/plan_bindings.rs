use anyhow::{anyhow, Result};
use prism_ir::PlanBinding;

pub(crate) fn validate_authored_plan_binding<ConceptExists, ArtifactExists, OutcomeExists>(
    binding: &PlanBinding,
    mut concept_exists: ConceptExists,
    mut artifact_exists: ArtifactExists,
    mut outcome_exists: OutcomeExists,
) -> Result<()>
where
    ConceptExists: FnMut(&str) -> bool,
    ArtifactExists: FnMut(&str) -> bool,
    OutcomeExists: FnMut(&str) -> bool,
{
    validate_concept_handles(&binding.concept_handles, &mut concept_exists)?;
    validate_published_refs("artifact_refs", &binding.artifact_refs, "artifact:")?;
    validate_published_refs("memory_refs", &binding.memory_refs, "memory:")?;
    validate_published_refs("outcome_refs", &binding.outcome_refs, "outcome:")?;
    validate_existing_refs(
        "artifact_refs",
        &binding.artifact_refs,
        &mut artifact_exists,
    )?;
    validate_existing_refs("outcome_refs", &binding.outcome_refs, &mut outcome_exists)?;
    Ok(())
}

fn validate_concept_handles<F>(handles: &[String], concept_exists: &mut F) -> Result<()>
where
    F: FnMut(&str) -> bool,
{
    for handle in handles {
        let trimmed = handle.trim();
        if trimmed.is_empty() {
            return Err(anyhow!(
                "authored plan binding `concept_handles` may not contain empty values"
            ));
        }
        if is_runtime_handle(trimmed) {
            return Err(anyhow!(
                "authored plan binding `concept_handles` must use stable concept handles, not runtime-only handles like `{trimmed}`"
            ));
        }
        if !trimmed.starts_with("concept://") {
            return Err(anyhow!(
                "authored plan binding `concept_handles` must use stable `concept://...` handles; got `{trimmed}`"
            ));
        }
        if !concept_exists(trimmed) {
            return Err(anyhow!(
                "authored plan binding `concept_handles` must reference an existing concept handle; got `{trimmed}`"
            ));
        }
    }
    Ok(())
}

fn validate_published_refs(field: &str, refs: &[String], prefix: &str) -> Result<()> {
    for value in refs {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(anyhow!(
                "authored plan binding `{field}` may not contain empty values"
            ));
        }
        if is_runtime_handle(trimmed) {
            return Err(anyhow!(
                "authored plan binding `{field}` must use stable published refs, not runtime-only handles like `{trimmed}`"
            ));
        }
        if !trimmed.starts_with(prefix) {
            return Err(anyhow!(
                "authored plan binding `{field}` must use stable `{prefix}...` refs; got `{trimmed}`"
            ));
        }
    }
    Ok(())
}

fn validate_existing_refs<F>(field: &str, refs: &[String], exists: &mut F) -> Result<()>
where
    F: FnMut(&str) -> bool,
{
    for value in refs {
        let trimmed = value.trim();
        if !exists(trimmed) {
            return Err(anyhow!(
                "authored plan binding `{field}` must reference an existing published ref; got `{trimmed}`"
            ));
        }
    }
    Ok(())
}

fn is_runtime_handle(value: &str) -> bool {
    value.starts_with("handle:")
}
