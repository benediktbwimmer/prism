use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::{AnchorRef, NodeId};
use prism_projections::{
    ConceptPacket, ConceptPublicationStatus, ConceptRelation, ConceptRelationKind, ConceptScope,
    ContractCompatibility, ContractGuarantee, ContractGuaranteeStrength, ContractKind,
    ContractPacket, ContractPublicationStatus, ContractStability, ContractStatus, ContractTarget,
    ContractValidation,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::published_plans::HydratedCoordinationPlanState;

mod export;
mod repo_state;

const ARCHITECTURE_HANDLE: &str = "concept://prism_architecture";
const ROOT_SUBSYSTEM_LIMIT: usize = 15;
const ROOT_KEY_CONCEPT_LIMIT: usize = 12;
const PUBLISHED_PROJECTION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrismDocSyncStatus {
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrismDocFileSync {
    pub path: PathBuf,
    pub status: PrismDocSyncStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrismDocSyncResult {
    pub status: PrismDocSyncStatus,
    pub files: Vec<PrismDocFileSync>,
}

pub use export::{PrismDocBundleFormat, PrismDocExportBundle, PrismDocExportResult};

pub fn render_repo_published_plan_markdown(
    snapshot: &prism_coordination::CoordinationSnapshotV2,
    plan_id: &prism_ir::PlanId,
    status: Option<prism_ir::PlanStatus>,
) -> Option<String> {
    repo_state::render_published_plan_markdown(snapshot, plan_id, status)
}

pub(crate) fn bundle_prism_doc_export(
    output_root: &Path,
    files: &[PrismDocFileSync],
    format: PrismDocBundleFormat,
) -> Result<PrismDocExportBundle> {
    export::write_bundle(output_root, files, format)
}

pub(crate) fn export_repo_prism_doc_with_plan_state(
    workspace_root: &Path,
    output_root: &Path,
    concepts: &[ConceptPacket],
    relations: &[ConceptRelation],
    contracts: &[ContractPacket],
    plan_state: Option<HydratedCoordinationPlanState>,
) -> Result<PrismDocSyncResult> {
    let state_catalog = repo_state::RepoStateCatalog::load(workspace_root, plan_state)?;
    let catalog = PrismDocCatalog::new(concepts, relations, contracts, state_catalog.summary());
    let prism_docs_dir = output_root.join("docs").join("prism");
    fs::create_dir_all(&prism_docs_dir)?;

    let mut files = Vec::new();
    files.push(write_generated_file(
        output_root.join("PRISM.md"),
        render_root_prism_doc(&catalog),
    )?);
    files.push(write_generated_file(
        prism_docs_dir.join("concepts.md"),
        render_concepts_doc(&catalog),
    )?);
    files.push(write_generated_file(
        prism_docs_dir.join("relations.md"),
        render_relations_doc(&catalog),
    )?);
    files.push(write_generated_file(
        prism_docs_dir.join("contracts.md"),
        render_contracts_doc(&catalog),
    )?);
    files.extend(repo_state::export_repo_state_docs(
        output_root,
        &state_catalog,
    )?);

    let status = if files
        .iter()
        .any(|file| file.status == PrismDocSyncStatus::Updated)
    {
        PrismDocSyncStatus::Updated
    } else {
        PrismDocSyncStatus::Unchanged
    };

    Ok(PrismDocSyncResult { status, files })
}

#[derive(Debug, Clone)]
struct PrismDocCatalog {
    concepts: Vec<ConceptPacket>,
    relations: Vec<ConceptRelation>,
    contracts: Vec<ContractPacket>,
    repo_state_summary: repo_state::RepoStateSummary,
    concept_names: HashMap<String, String>,
    relation_degree: HashMap<String, usize>,
    metadata: PublishedProjectionMetadata,
}

impl PrismDocCatalog {
    fn new(
        concepts: &[ConceptPacket],
        relations: &[ConceptRelation],
        contracts: &[ContractPacket],
        repo_state_summary: repo_state::RepoStateSummary,
    ) -> Self {
        let concepts = active_repo_concepts(concepts);
        let concept_names = concepts
            .iter()
            .map(|concept| {
                (
                    concept.handle.clone(),
                    format!("`{}` (`{}`)", concept.canonical_name, concept.handle),
                )
            })
            .collect::<HashMap<_, _>>();
        let relations = visible_repo_relations(relations, &concept_names);
        let contracts = active_repo_contracts(contracts);
        let relation_degree = relation_degree_map(&relations);
        let metadata = PublishedProjectionMetadata::from_sources(&concepts, &relations, &contracts);
        Self {
            concepts,
            relations,
            contracts,
            repo_state_summary,
            concept_names,
            relation_degree,
            metadata,
        }
    }

    fn architecture_concept(&self) -> Option<&ConceptPacket> {
        self.concepts
            .iter()
            .find(|concept| concept.handle == ARCHITECTURE_HANDLE)
    }

    fn top_subsystems(&self) -> Vec<&ConceptPacket> {
        let mut handles = self
            .relations
            .iter()
            .filter_map(|relation| architecture_neighbor_handle(relation))
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|handle| *handle != ARCHITECTURE_HANDLE)
            .collect::<Vec<_>>();
        handles.sort_by(|left, right| {
            self.relation_degree
                .get(*right)
                .copied()
                .unwrap_or(0)
                .cmp(&self.relation_degree.get(*left).copied().unwrap_or(0))
                .then_with(|| {
                    concept_name_for_handle(&self.concepts, left)
                        .cmp(concept_name_for_handle(&self.concepts, right))
                })
        });
        handles.truncate(ROOT_SUBSYSTEM_LIMIT);
        handles
            .into_iter()
            .filter_map(|handle| {
                self.concepts
                    .iter()
                    .find(|concept| concept.handle == handle)
            })
            .collect()
    }

    fn key_concepts(&self) -> Vec<&ConceptPacket> {
        let subsystem_handles = self
            .top_subsystems()
            .into_iter()
            .map(|concept| concept.handle.clone())
            .collect::<HashSet<_>>();
        let mut concepts = self
            .concepts
            .iter()
            .filter(|concept| {
                concept.handle != ARCHITECTURE_HANDLE
                    && !subsystem_handles.contains(&concept.handle)
            })
            .collect::<Vec<_>>();
        concepts.sort_by(|left, right| {
            self.relation_degree
                .get(&right.handle)
                .copied()
                .unwrap_or(0)
                .cmp(&self.relation_degree.get(&left.handle).copied().unwrap_or(0))
                .then_with(|| {
                    left.canonical_name
                        .to_ascii_lowercase()
                        .cmp(&right.canonical_name.to_ascii_lowercase())
                })
                .then_with(|| left.handle.cmp(&right.handle))
        });
        concepts.truncate(ROOT_KEY_CONCEPT_LIMIT);
        concepts
    }
}

#[derive(Debug, Clone)]
struct PublishedProjectionMetadata {
    projection_version: u32,
    source_head: String,
    source_logical_timestamp: Option<u64>,
    concept_count: usize,
    relation_count: usize,
    contract_count: usize,
}

#[derive(Serialize)]
struct PublishedProjectionDigestInput<'a> {
    concepts: &'a [ConceptPacket],
    relations: &'a [ConceptRelation],
    contracts: &'a [ContractPacket],
}

impl PublishedProjectionMetadata {
    fn from_sources(
        concepts: &[ConceptPacket],
        relations: &[ConceptRelation],
        contracts: &[ContractPacket],
    ) -> Self {
        let digest_input = PublishedProjectionDigestInput {
            concepts,
            relations,
            contracts,
        };
        let canonical = serde_jcs::to_vec(&digest_input).expect("projection stamp should encode");
        let source_head = format!("sha256:{:x}", Sha256::digest(canonical));
        let source_logical_timestamp = concepts
            .iter()
            .filter_map(|concept| concept.publication.as_ref())
            .flat_map(|publication| {
                [Some(publication.published_at), publication.last_reviewed_at].into_iter()
            })
            .chain(
                contracts
                    .iter()
                    .filter_map(|contract| contract.publication.as_ref())
                    .flat_map(|publication| {
                        [Some(publication.published_at), publication.last_reviewed_at].into_iter()
                    }),
            )
            .flatten()
            .max();

        Self {
            projection_version: PUBLISHED_PROJECTION_VERSION,
            source_head,
            source_logical_timestamp,
            concept_count: concepts.len(),
            relation_count: relations.len(),
            contract_count: contracts.len(),
        }
    }
}

fn render_root_prism_doc(catalog: &PrismDocCatalog) -> String {
    let mut markdown = String::new();
    markdown.push_str("# PRISM\n\n");
    markdown.push_str(
        "> This file is generated from repo-scoped PRISM knowledge. The concise summary lives here,\n",
    );
    markdown.push_str("> while the full generated catalog lives under `docs/prism/`.\n\n");
    write_projection_metadata_section(&mut markdown, &catalog.metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Active repo concepts: {}\n",
        catalog.concepts.len()
    ));
    markdown.push_str(&format!(
        "- Active repo relations: {}\n",
        catalog.relations.len()
    ));
    markdown.push_str(&format!(
        "- Active repo contracts: {}\n",
        catalog.contracts.len()
    ));
    markdown.push_str(&format!(
        "- Active repo memories: {}\n",
        catalog.repo_state_summary.memory_count
    ));
    markdown.push_str(&format!(
        "- Published plans: {}\n",
        catalog.repo_state_summary.plan_count
    ));
    markdown.push_str("- Full concept catalog: `docs/prism/concepts.md`\n");
    markdown.push_str("- Full relation catalog: `docs/prism/relations.md`\n");
    markdown.push_str("- Full contract catalog: `docs/prism/contracts.md`\n");
    markdown.push_str("- Published memory catalog: `docs/prism/memory.md`\n");
    markdown.push_str("- Published plan catalog: `docs/prism/plans/index.md`\n\n");

    markdown.push_str("## How to Read This Repo\n\n");
    markdown.push_str("- Start with this file for the main architecture map and the most central repo concepts.\n");
    markdown.push_str(
        "- Use `docs/prism/concepts.md` when you need the full generated concept encyclopedia.\n",
    );
    markdown.push_str(
        "- Use `docs/prism/relations.md` when you need the typed concept-to-concept graph.\n",
    );
    markdown.push_str(
        "- Use `docs/prism/contracts.md` when you need published guarantees, assumptions, validations, and compatibility guidance.\n",
    );
    markdown.push_str(
        "- Use `docs/prism/memory.md` when you need the current repo-published memory surface.\n",
    );
    markdown.push_str(
        "- Use `docs/prism/plans/index.md` when you need the current published plan catalog and per-plan markdown projections.\n",
    );
    markdown.push_str(
        "- Treat tracked `.prism/state/**` snapshot shards plus `.prism/state/manifest.json` as the current repo-published source of truth; the legacy tracked `.jsonl` streams are migration-era compatibility inputs, and these markdown files are derived artifacts.\n\n",
    );

    if let Some(architecture) = catalog.architecture_concept() {
        markdown.push_str("## Architecture\n\n");
        markdown.push_str(&format!(
            "- `{}` (`{}`): {}\n\n",
            architecture.canonical_name, architecture.handle, architecture.summary
        ));
    }

    let subsystems = catalog.top_subsystems();
    if !subsystems.is_empty() {
        markdown.push_str("## Subsystem Map\n\n");
        for concept in subsystems {
            markdown.push_str(&format!(
                "- `{}` (`{}`): {}\n",
                concept.canonical_name, concept.handle, concept.summary
            ));
        }
        markdown.push('\n');
    }

    let key_concepts = catalog.key_concepts();
    if !key_concepts.is_empty() {
        markdown.push_str("## Key Concepts\n\n");
        for concept in key_concepts {
            markdown.push_str(&format!(
                "- `{}` (`{}`): {}\n",
                concept.canonical_name, concept.handle, concept.summary
            ));
        }
        markdown.push('\n');
    }

    markdown.push_str("## Generated Docs\n\n");
    markdown.push_str("- `docs/prism/concepts.md`: full concept catalog with members, evidence, and risk hints.\n");
    markdown.push_str(
        "- `docs/prism/relations.md`: full typed relation catalog with evidence and confidence.\n",
    );
    markdown.push_str(
        "- `docs/prism/contracts.md`: full contract catalog with guarantees, assumptions, validations, and compatibility guidance.\n",
    );
    markdown.push_str(
        "- `docs/prism/memory.md`: current repo-published memory entries with anchors, provenance, and trust.\n",
    );
    markdown.push_str(
        "- `docs/prism/plans/index.md`: published plan catalog plus per-plan markdown projections under `docs/prism/plans/`.\n",
    );
    markdown
}

fn render_concepts_doc(catalog: &PrismDocCatalog) -> String {
    let mut markdown = String::new();
    markdown.push_str("# PRISM Concepts\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM concept and relation knowledge.\n");
    markdown.push_str("> Return to the concise entrypoint in `../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &catalog.metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Active repo concepts: {}\n",
        catalog.concepts.len()
    ));
    markdown.push_str(&format!(
        "- Active repo relations: {}\n\n",
        catalog.relations.len()
    ));
    markdown.push_str(&format!(
        "- Active repo contracts: {}\n\n",
        catalog.contracts.len()
    ));

    if catalog.concepts.is_empty() {
        markdown.push_str("No active repo-scoped concepts are currently published.\n");
        return markdown;
    }

    markdown.push_str("## Published Concepts\n\n");
    for concept in &catalog.concepts {
        markdown.push_str(&format!(
            "- `{}` (`{}`): {}\n",
            concept.canonical_name, concept.handle, concept.summary
        ));
    }
    markdown.push('\n');

    for concept in &catalog.concepts {
        markdown.push_str(&format!("## {}\n\n", concept.canonical_name));
        markdown.push_str(&format!("Handle: `{}`\n\n", concept.handle));
        markdown.push_str(&format!("{}\n\n", concept.summary));

        if !concept.aliases.is_empty() {
            markdown.push_str("Aliases: ");
            markdown.push_str(&join_inline_code(&concept.aliases));
            markdown.push_str("\n\n");
        }

        write_node_section(&mut markdown, "Core Members", &concept.core_members);
        write_node_section(
            &mut markdown,
            "Supporting Members",
            &concept.supporting_members,
        );
        write_node_section(&mut markdown, "Likely Tests", &concept.likely_tests);

        let related = concept_relation_lines(&concept.handle, catalog);
        if !related.is_empty() {
            markdown.push_str("### Related Concepts\n\n");
            for line in related {
                markdown.push_str("- ");
                markdown.push_str(&line);
                markdown.push('\n');
            }
            markdown.push('\n');
        }

        if !concept.evidence.is_empty() {
            markdown.push_str("### Evidence\n\n");
            for line in &concept.evidence {
                markdown.push_str("- ");
                markdown.push_str(line);
                markdown.push('\n');
            }
            markdown.push('\n');
        }

        if let Some(risk_hint) = concept.risk_hint.as_ref() {
            markdown.push_str("### Risk Hint\n\n");
            markdown.push_str("- ");
            markdown.push_str(risk_hint);
            markdown.push_str("\n\n");
        }
    }

    markdown
}

fn render_relations_doc(catalog: &PrismDocCatalog) -> String {
    let mut markdown = String::new();
    markdown.push_str("# PRISM Relations\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM concept relations.\n");
    markdown.push_str("> Return to the concise entrypoint in `../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &catalog.metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Active repo relations: {}\n",
        catalog.relations.len()
    ));
    markdown.push_str(&format!(
        "- Active repo concepts covered: {}\n\n",
        catalog.concepts.len()
    ));
    markdown.push_str(&format!(
        "- Active repo contracts: {}\n\n",
        catalog.contracts.len()
    ));

    if catalog.relations.is_empty() {
        markdown.push_str("No active repo-scoped concept relations are currently published.\n");
        return markdown;
    }

    let mut concepts_with_relations = catalog
        .concepts
        .iter()
        .filter(|concept| {
            catalog
                .relations
                .iter()
                .any(|relation| relation.source_handle == concept.handle)
        })
        .collect::<Vec<_>>();
    concepts_with_relations.sort_by(|left, right| {
        left.canonical_name
            .to_ascii_lowercase()
            .cmp(&right.canonical_name.to_ascii_lowercase())
            .then_with(|| left.handle.cmp(&right.handle))
    });

    for concept in concepts_with_relations {
        markdown.push_str(&format!("## {}\n\n", concept.canonical_name));
        markdown.push_str(&format!("Source Handle: `{}`\n\n", concept.handle));
        for relation in catalog
            .relations
            .iter()
            .filter(|relation| relation.source_handle == concept.handle)
        {
            let target = catalog
                .concept_names
                .get(&relation.target_handle)
                .cloned()
                .unwrap_or_else(|| format!("`{}`", relation.target_handle));
            markdown.push_str(&format!(
                "- {}: {} (confidence {:.2})\n",
                relation_kind_label(relation.kind),
                target,
                relation.confidence
            ));
            if !relation.evidence.is_empty() {
                for evidence in &relation.evidence {
                    markdown.push_str("  evidence: ");
                    markdown.push_str(evidence);
                    markdown.push('\n');
                }
            }
        }
        markdown.push('\n');
    }

    markdown
}

fn render_contracts_doc(catalog: &PrismDocCatalog) -> String {
    let mut markdown = String::new();
    markdown.push_str("# PRISM Contracts\n\n");
    markdown.push_str("> Generated from repo-scoped PRISM contract knowledge.\n");
    markdown.push_str("> Return to the concise entrypoint in `../../PRISM.md`.\n\n");
    write_projection_metadata_section(&mut markdown, &catalog.metadata);

    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!(
        "- Active repo contracts: {}\n",
        catalog.contracts.len()
    ));
    markdown.push_str(&format!(
        "- Active repo concepts: {}\n",
        catalog.concepts.len()
    ));
    markdown.push_str(&format!(
        "- Active repo relations: {}\n\n",
        catalog.relations.len()
    ));

    if catalog.contracts.is_empty() {
        markdown.push_str("No active repo-scoped contracts are currently published.\n");
        return markdown;
    }

    markdown.push_str("## Published Contracts\n\n");
    for contract in &catalog.contracts {
        markdown.push_str(&format!(
            "- `{}` (`{}`): {}\n",
            contract.name, contract.handle, contract.summary
        ));
    }
    markdown.push('\n');

    for contract in &catalog.contracts {
        markdown.push_str(&format!("## {}\n\n", contract.name));
        markdown.push_str(&format!("Handle: `{}`\n\n", contract.handle));
        markdown.push_str(&format!("{}\n\n", contract.summary));
        markdown.push_str(&format!(
            "Kind: {}  \nStatus: {}  \nStability: {}\n\n",
            contract_kind_label(contract.kind),
            contract_status_label(contract.status),
            contract_stability_label(contract.stability)
        ));

        if !contract.aliases.is_empty() {
            markdown.push_str("Aliases: ");
            markdown.push_str(&join_inline_code(&contract.aliases));
            markdown.push_str("\n\n");
        }

        write_contract_target_section(&mut markdown, "Subject", &contract.subject);
        write_contract_guarantees_section(&mut markdown, &contract.guarantees);
        write_string_section(&mut markdown, "Assumptions", &contract.assumptions);
        write_contract_targets_section(&mut markdown, "Consumers", &contract.consumers);
        write_contract_validations_section(&mut markdown, &contract.validations);
        write_contract_compatibility_section(&mut markdown, &contract.compatibility);
        write_string_section(&mut markdown, "Evidence", &contract.evidence);
    }

    markdown
}

fn write_projection_metadata_section(
    markdown: &mut String,
    metadata: &PublishedProjectionMetadata,
) {
    markdown.push_str("## Projection Metadata\n\n");
    markdown.push_str("- Projection class: `published`\n");
    markdown.push_str("- Authority planes: `published_repo`\n");
    markdown.push_str(&format!(
        "- Projection version: `{}`\n",
        metadata.projection_version
    ));
    markdown.push_str(&format!("- Source head: `{}`\n", metadata.source_head));
    if let Some(timestamp) = metadata.source_logical_timestamp {
        markdown.push_str(&format!("- Source logical timestamp: `{timestamp}`\n"));
    } else {
        markdown.push_str("- Source logical timestamp: `unknown`\n");
    }
    markdown.push_str(&format!(
        "- Source snapshot: `{}` concepts, `{}` relations, `{}` contracts\n\n",
        metadata.concept_count, metadata.relation_count, metadata.contract_count
    ));
}

fn write_generated_file(path: PathBuf, rendered: String) -> Result<PrismDocFileSync> {
    let existing = fs::read_to_string(&path).ok();
    if existing.as_deref() == Some(rendered.as_str()) {
        return Ok(PrismDocFileSync {
            path,
            status: PrismDocSyncStatus::Unchanged,
        });
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, rendered)?;
    Ok(PrismDocFileSync {
        path,
        status: PrismDocSyncStatus::Updated,
    })
}

fn active_repo_concepts(concepts: &[ConceptPacket]) -> Vec<ConceptPacket> {
    let mut concepts = concepts
        .iter()
        .filter(|concept| {
            concept.scope == ConceptScope::Repo
                && concept.publication.as_ref().is_none_or(|publication| {
                    publication.status != ConceptPublicationStatus::Retired
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    concepts.sort_by(|left, right| {
        left.canonical_name
            .to_ascii_lowercase()
            .cmp(&right.canonical_name.to_ascii_lowercase())
            .then_with(|| left.handle.cmp(&right.handle))
    });
    concepts
}

fn active_repo_contracts(contracts: &[ContractPacket]) -> Vec<ContractPacket> {
    let mut contracts = contracts
        .iter()
        .filter(|contract| {
            contract.scope == ConceptScope::Repo
                && contract.status != ContractStatus::Retired
                && contract.publication.as_ref().is_none_or(|publication| {
                    publication.status != ContractPublicationStatus::Retired
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    contracts.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.handle.cmp(&right.handle))
    });
    contracts
}

fn visible_repo_relations(
    relations: &[ConceptRelation],
    concepts_by_handle: &HashMap<String, String>,
) -> Vec<ConceptRelation> {
    let mut relations = relations
        .iter()
        .filter(|relation| {
            relation.scope == ConceptScope::Repo
                && concepts_by_handle.contains_key(&relation.source_handle)
                && concepts_by_handle.contains_key(&relation.target_handle)
        })
        .cloned()
        .collect::<Vec<_>>();
    relations.sort_by(|left, right| {
        left.source_handle
            .cmp(&right.source_handle)
            .then_with(|| left.target_handle.cmp(&right.target_handle))
            .then_with(|| relation_kind_label(left.kind).cmp(relation_kind_label(right.kind)))
    });
    relations
}

fn relation_degree_map(relations: &[ConceptRelation]) -> HashMap<String, usize> {
    let mut degrees = HashMap::<String, usize>::new();
    for relation in relations {
        *degrees.entry(relation.source_handle.clone()).or_insert(0) += 1;
        *degrees.entry(relation.target_handle.clone()).or_insert(0) += 1;
    }
    degrees
}

fn architecture_neighbor_handle(relation: &ConceptRelation) -> Option<&str> {
    if relation.kind != ConceptRelationKind::PartOf {
        return None;
    }
    if relation.target_handle == ARCHITECTURE_HANDLE {
        Some(relation.source_handle.as_str())
    } else if relation.source_handle == ARCHITECTURE_HANDLE {
        Some(relation.target_handle.as_str())
    } else {
        None
    }
}

fn concept_name_for_handle<'a>(concepts: &'a [ConceptPacket], handle: &'a str) -> &'a str {
    concepts
        .iter()
        .find(|concept| concept.handle == handle)
        .map(|concept| concept.canonical_name.as_str())
        .unwrap_or(handle)
}

fn concept_relation_lines(handle: &str, catalog: &PrismDocCatalog) -> Vec<String> {
    let mut lines = catalog
        .relations
        .iter()
        .filter_map(|relation| {
            if relation.source_handle == handle {
                let other = catalog.concept_names.get(&relation.target_handle)?;
                Some(format!("{}: {}", relation_kind_label(relation.kind), other))
            } else if relation.target_handle == handle {
                let other = catalog.concept_names.get(&relation.source_handle)?;
                Some(format!(
                    "{}: {}",
                    reverse_relation_kind_label(relation.kind),
                    other
                ))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

fn write_node_section(markdown: &mut String, title: &str, nodes: &[NodeId]) {
    if nodes.is_empty() {
        return;
    }
    markdown.push_str("### ");
    markdown.push_str(title);
    markdown.push_str("\n\n");
    for node in nodes {
        markdown.push_str("- `");
        markdown.push_str(&node.path);
        markdown.push_str("`\n");
    }
    markdown.push('\n');
}

fn write_contract_target_section(markdown: &mut String, title: &str, target: &ContractTarget) {
    if target.anchors.is_empty() && target.concept_handles.is_empty() {
        return;
    }
    markdown.push_str("### ");
    markdown.push_str(title);
    markdown.push_str("\n\n");
    if !target.anchors.is_empty() {
        markdown.push_str("Anchors:\n");
        for anchor in &target.anchors {
            markdown.push_str("- `");
            markdown.push_str(&anchor_label(anchor));
            markdown.push_str("`\n");
        }
    }
    if !target.concept_handles.is_empty() {
        markdown.push_str("Concept Handles:\n");
        for handle in &target.concept_handles {
            markdown.push_str("- `");
            markdown.push_str(handle);
            markdown.push_str("`\n");
        }
    }
    markdown.push('\n');
}

fn write_contract_targets_section(markdown: &mut String, title: &str, targets: &[ContractTarget]) {
    if targets.is_empty() {
        return;
    }
    markdown.push_str("### ");
    markdown.push_str(title);
    markdown.push_str("\n\n");
    for (index, target) in targets.iter().enumerate() {
        markdown.push_str(&format!("#### Target {}\n\n", index + 1));
        write_contract_target_contents(markdown, target);
    }
}

fn write_contract_target_contents(markdown: &mut String, target: &ContractTarget) {
    if !target.anchors.is_empty() {
        markdown.push_str("Anchors:\n");
        for anchor in &target.anchors {
            markdown.push_str("- `");
            markdown.push_str(&anchor_label(anchor));
            markdown.push_str("`\n");
        }
    }
    if !target.concept_handles.is_empty() {
        markdown.push_str("Concept Handles:\n");
        for handle in &target.concept_handles {
            markdown.push_str("- `");
            markdown.push_str(handle);
            markdown.push_str("`\n");
        }
    }
    markdown.push('\n');
}

fn write_contract_guarantees_section(markdown: &mut String, guarantees: &[ContractGuarantee]) {
    if guarantees.is_empty() {
        return;
    }
    markdown.push_str("### Guarantees\n\n");
    for guarantee in guarantees {
        markdown.push_str("- `");
        markdown.push_str(&guarantee.id);
        markdown.push_str("`: ");
        markdown.push_str(&guarantee.statement);
        if let Some(scope) = guarantee.scope.as_ref() {
            markdown.push_str(" (scope: ");
            markdown.push_str(scope);
            markdown.push(')');
        }
        if let Some(strength) = guarantee.strength {
            markdown.push_str(" [");
            markdown.push_str(contract_guarantee_strength_label(strength));
            markdown.push(']');
        }
        markdown.push('\n');
        for evidence_ref in &guarantee.evidence_refs {
            markdown.push_str("  evidence ref: `");
            markdown.push_str(evidence_ref);
            markdown.push_str("`\n");
        }
    }
    markdown.push('\n');
}

fn write_contract_validations_section(markdown: &mut String, validations: &[ContractValidation]) {
    if validations.is_empty() {
        return;
    }
    markdown.push_str("### Validations\n\n");
    for validation in validations {
        markdown.push_str("- `");
        markdown.push_str(&validation.id);
        markdown.push('`');
        if let Some(summary) = validation.summary.as_ref() {
            markdown.push_str(": ");
            markdown.push_str(summary);
        }
        markdown.push('\n');
        for anchor in &validation.anchors {
            markdown.push_str("  anchor: `");
            markdown.push_str(&anchor_label(anchor));
            markdown.push_str("`\n");
        }
    }
    markdown.push('\n');
}

fn write_contract_compatibility_section(
    markdown: &mut String,
    compatibility: &ContractCompatibility,
) {
    if compatibility.compatible.is_empty()
        && compatibility.additive.is_empty()
        && compatibility.risky.is_empty()
        && compatibility.breaking.is_empty()
        && compatibility.migrating.is_empty()
    {
        return;
    }
    markdown.push_str("### Compatibility\n\n");
    write_string_subsection(markdown, "Compatible", &compatibility.compatible);
    write_string_subsection(markdown, "Additive", &compatibility.additive);
    write_string_subsection(markdown, "Risky", &compatibility.risky);
    write_string_subsection(markdown, "Breaking", &compatibility.breaking);
    write_string_subsection(markdown, "Migrating", &compatibility.migrating);
}

fn write_string_section(markdown: &mut String, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    markdown.push_str("### ");
    markdown.push_str(title);
    markdown.push_str("\n\n");
    for value in values {
        markdown.push_str("- ");
        markdown.push_str(value);
        markdown.push('\n');
    }
    markdown.push('\n');
}

fn write_string_subsection(markdown: &mut String, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    markdown.push_str("#### ");
    markdown.push_str(title);
    markdown.push_str("\n\n");
    for value in values {
        markdown.push_str("- ");
        markdown.push_str(value);
        markdown.push('\n');
    }
    markdown.push('\n');
}

fn join_inline_code(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn anchor_label(anchor: &AnchorRef) -> String {
    match anchor {
        AnchorRef::Node(node) => format!("node:{}:{}:{}", node.crate_name, node.path, node.kind),
        AnchorRef::Lineage(lineage) => format!("lineage:{}", lineage.0),
        AnchorRef::File(file) => format!("file:{}", file.0),
        AnchorRef::Kind(kind) => format!("kind:{kind}"),
    }
}

fn contract_kind_label(kind: ContractKind) -> &'static str {
    match kind {
        ContractKind::Interface => "interface",
        ContractKind::Behavioral => "behavioral",
        ContractKind::DataShape => "data shape",
        ContractKind::DependencyBoundary => "dependency boundary",
        ContractKind::Lifecycle => "lifecycle",
        ContractKind::Protocol => "protocol",
        ContractKind::Operational => "operational",
    }
}

fn contract_status_label(status: ContractStatus) -> &'static str {
    match status {
        ContractStatus::Candidate => "candidate",
        ContractStatus::Active => "active",
        ContractStatus::Deprecated => "deprecated",
        ContractStatus::Retired => "retired",
    }
}

fn contract_stability_label(stability: ContractStability) -> &'static str {
    match stability {
        ContractStability::Experimental => "experimental",
        ContractStability::Internal => "internal",
        ContractStability::Public => "public",
        ContractStability::Deprecated => "deprecated",
        ContractStability::Migrating => "migrating",
    }
}

fn contract_guarantee_strength_label(strength: ContractGuaranteeStrength) -> &'static str {
    match strength {
        ContractGuaranteeStrength::Hard => "hard",
        ContractGuaranteeStrength::Soft => "soft",
        ContractGuaranteeStrength::Conditional => "conditional",
    }
}

fn relation_kind_label(kind: ConceptRelationKind) -> &'static str {
    match kind {
        ConceptRelationKind::DependsOn => "depends on",
        ConceptRelationKind::Specializes => "specializes",
        ConceptRelationKind::PartOf => "part of",
        ConceptRelationKind::ValidatedBy => "validated by",
        ConceptRelationKind::OftenUsedWith => "often used with",
        ConceptRelationKind::Supersedes => "supersedes",
        ConceptRelationKind::ConfusedWith => "confused with",
    }
}

fn reverse_relation_kind_label(kind: ConceptRelationKind) -> &'static str {
    match kind {
        ConceptRelationKind::DependsOn => "depended on by",
        ConceptRelationKind::Specializes => "specialized by",
        ConceptRelationKind::PartOf => "has part",
        ConceptRelationKind::ValidatedBy => "validates",
        ConceptRelationKind::OftenUsedWith => "often used with",
        ConceptRelationKind::Supersedes => "superseded by",
        ConceptRelationKind::ConfusedWith => "confused with",
    }
}
