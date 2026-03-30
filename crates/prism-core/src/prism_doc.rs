use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use prism_ir::NodeId;
use prism_projections::{
    ConceptPacket, ConceptPublicationStatus, ConceptRelation, ConceptRelationKind, ConceptScope,
};

use crate::util::prism_doc_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrismDocSyncStatus {
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrismDocSyncResult {
    pub path: PathBuf,
    pub status: PrismDocSyncStatus,
}

pub(crate) fn sync_repo_prism_doc(
    root: &Path,
    concepts: &[ConceptPacket],
    relations: &[ConceptRelation],
) -> Result<PrismDocSyncResult> {
    let path = prism_doc_path(root);
    let rendered = render_repo_prism_doc(concepts, relations);
    let existing = fs::read_to_string(&path).ok();
    if existing.as_deref() == Some(rendered.as_str()) {
        return Ok(PrismDocSyncResult {
            path,
            status: PrismDocSyncStatus::Unchanged,
        });
    }
    fs::write(&path, rendered)?;
    Ok(PrismDocSyncResult {
        path,
        status: PrismDocSyncStatus::Updated,
    })
}

fn render_repo_prism_doc(concepts: &[ConceptPacket], relations: &[ConceptRelation]) -> String {
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

    let mut markdown = String::new();
    markdown.push_str("# PRISM\n\n");
    markdown.push_str(
        "> This file is generated from repo-scoped PRISM knowledge in `.prism/concepts/events.jsonl`\n",
    );
    markdown.push_str(
        "> and `.prism/concepts/relations.jsonl`. Regenerate on demand with `prism docs generate`.\n\n",
    );
    markdown.push_str("## Overview\n\n");
    markdown.push_str(&format!("- Active repo concepts: {}\n", concepts.len()));
    markdown.push_str(&format!("- Active repo relations: {}\n\n", relations.len()));

    if concepts.is_empty() {
        markdown.push_str("No active repo-scoped concepts are currently published.\n");
        return markdown;
    }

    markdown.push_str("## Published Concepts\n\n");
    for concept in &concepts {
        markdown.push_str(&format!(
            "- `{}` (`{}`): {}\n",
            concept.canonical_name, concept.handle, concept.summary
        ));
    }
    markdown.push('\n');

    for concept in &concepts {
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

        let related = concept_relation_lines(&concept.handle, &relations, &concept_names);
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

fn concept_relation_lines(
    handle: &str,
    relations: &[ConceptRelation],
    concepts_by_handle: &HashMap<String, String>,
) -> Vec<String> {
    let mut lines = relations
        .iter()
        .filter_map(|relation| {
            if relation.source_handle == handle {
                let other = concepts_by_handle.get(&relation.target_handle)?;
                Some(format!("{}: {}", relation_kind_label(relation.kind), other))
            } else if relation.target_handle == handle {
                let other = concepts_by_handle.get(&relation.source_handle)?;
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

fn join_inline_code(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ")
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
