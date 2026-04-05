use std::collections::HashSet;
use std::time::Instant;

use prism_ir::{AnchorRef, NodeId};
use prism_js::{
    AgentConceptPacketView, AgentConceptResultView, AgentSuggestedActionView,
    AgentTargetHandleView, AgentWorksetResultView, ConceptBindingMetadataView, ConceptDecodeView,
    ConceptProvenanceView, ConceptPublicationStatusView, ConceptPublicationView,
    ConceptResolutionView, ConceptScopeView,
};
use prism_memory::{MemoryModule, OutcomeKind, OutcomeRecallQuery, RecallQuery};
use prism_query::{ConceptDecodeLens, ConceptPacket, ConceptPublicationStatus, ConceptScope};

use super::suggested_actions::{
    dedupe_suggested_actions, suggested_expand_action, suggested_open_action,
    suggested_workset_action,
};
use super::workset::budgeted_workset_result_with_followups;
use super::*;
use crate::{
    concept_decode_lens_view, concept_followthrough_targets, concept_packet_view,
    concept_relation_view, concept_resolution_is_ambiguous, concept_verbosity_view, recent_patches,
    resolve_concepts_for_task_context, scored_memory_view, symbol_views_for_ids,
    truncate_concept_relations, truncate_vec_with_omitted, validation_recipe_view_with,
    ConceptPacketTruncationStats, ConceptVerbosity,
};

const CONCEPT_PATCH_LIMIT: usize = 4;
const CONCEPT_MEMORY_LIMIT: usize = 4;
const CONCEPT_FAILURE_LIMIT: usize = 8;

#[derive(Debug, Clone)]
pub(super) struct CompactConceptSelection {
    pub(super) packet: ConceptPacket,
    pub(super) primary: AgentTargetHandleView,
    pub(super) supporting_reads: Vec<AgentTargetHandleView>,
    pub(super) likely_tests: Vec<AgentTargetHandleView>,
}

impl QueryHost {
    pub(crate) fn compact_concept(
        &self,
        session: Arc<SessionState>,
        args: PrismConceptArgs,
    ) -> Result<AgentConceptResultView> {
        let query_text = if let Some(handle) = args.handle.as_ref() {
            format!("prism_concept({handle})")
        } else if let Some(query) = args.query.as_ref() {
            format!("prism_concept({query})")
        } else {
            "prism_concept".to_string()
        };
        self.execute_compact_tool(
            Arc::clone(&session),
            "prism_concept",
            query_text,
            move |host, query_run| {
                let prism = host.current_prism();
                let resolve_started = Instant::now();
                let resolution = resolve_concept_packet(prism.as_ref(), session.as_ref(), &args)?;
                query_run.record_phase(
                    "concept.resolve",
                    &json!({
                        "mode": if args.handle.is_some() { "handle" } else { "query" },
                        "matchedHandle": resolution.packet.handle,
                    }),
                    resolve_started.elapsed(),
                    true,
                    None,
                );
                let packet = resolution.packet.clone();
                let verbosity = concept_verbosity(args.verbosity);
                let packet_view_started = Instant::now();
                let packet_view = agent_concept_packet_view(
                    session.as_ref(),
                    prism.as_ref(),
                    &packet,
                    verbosity,
                    Some(resolution.clone()),
                    args.include_binding_metadata.unwrap_or(false),
                )?;
                query_run.record_phase(
                    "concept.packetView",
                    &json!({
                        "verbosity": format!("{:?}", verbosity),
                        "coreMembers": packet.core_members.len(),
                        "supportingMembers": packet.supporting_members.len(),
                        "likelyTests": packet.likely_tests.len(),
                    }),
                    packet_view_started.elapsed(),
                    true,
                    None,
                );
                let alternates_started = Instant::now();
                let alternates = resolve_concept_alternates(
                    prism.as_ref(),
                    session.as_ref(),
                    &args,
                    packet.handle.as_str(),
                    verbosity,
                    args.include_binding_metadata.unwrap_or(false),
                )?;
                query_run.record_phase(
                    "concept.alternates",
                    &json!({
                        "count": alternates.len(),
                        "queryDriven": args.query.is_some(),
                    }),
                    alternates_started.elapsed(),
                    true,
                    None,
                );
                let decode_started = Instant::now();
                let decode = args
                    .lens
                    .as_ref()
                    .map(|lens| {
                        decode_concept(session.as_ref(), prism.as_ref(), &packet, lens, verbosity)
                    })
                    .transpose()?;
                query_run.record_phase(
                    "concept.decode",
                    &json!({
                        "requested": args.lens.is_some(),
                    }),
                    decode_started.elapsed(),
                    true,
                    None,
                );
                Ok((
                    AgentConceptResultView {
                        packet: packet_view,
                        decode,
                        alternates,
                    },
                    Vec::new(),
                ))
            },
        )
    }
}

fn resolve_concept_packet(
    prism: &Prism,
    session: &SessionState,
    args: &PrismConceptArgs,
) -> Result<prism_query::ConceptResolution> {
    match (args.handle.as_deref(), args.query.as_deref()) {
        (Some(handle), _) => prism
            .concept_by_handle(handle)
            .map(|packet| prism_query::ConceptResolution {
                packet,
                score: i32::MAX,
                reasons: vec!["handle exact match".to_string()],
            })
            .ok_or_else(|| anyhow!("no concept packet matched `{handle}`")),
        (None, Some(query)) => {
            resolve_concepts_for_task_context(prism, session, args.task_id.as_deref(), query, 1)
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no concept packet matched `{query}`"))
        }
        (None, None) => Err(anyhow!("prism_concept requires `handle` or `query`")),
    }
}

fn agent_concept_packet_view(
    session: &SessionState,
    prism: &Prism,
    packet: &ConceptPacket,
    verbosity: ConceptVerbosity,
    resolution: Option<prism_query::ConceptResolution>,
    include_binding_metadata: bool,
) -> Result<AgentConceptPacketView> {
    let (core_members, core_members_omitted) = truncate_vec_with_omitted(
        compact_handles_for_ids(session, prism, &packet.core_members)?,
        verbosity.max_member_count(),
    );
    let (supporting_members, supporting_members_omitted) = truncate_vec_with_omitted(
        compact_handles_for_ids(session, prism, &packet.supporting_members)?,
        verbosity.max_member_count(),
    );
    let (likely_tests, likely_tests_omitted) = truncate_vec_with_omitted(
        compact_handles_for_ids(session, prism, &packet.likely_tests)?,
        verbosity.max_member_count(),
    );
    let (evidence, evidence_omitted) =
        truncate_vec_with_omitted(packet.evidence.clone(), verbosity.max_evidence_count());
    let (relations, relation_truncation) = truncate_concept_relations(
        prism
            .concept_relations_for_handle(&packet.handle)
            .into_iter()
            .map(|relation| concept_relation_view(prism, &packet.handle, relation))
            .collect(),
        verbosity,
    );
    let truncation = ConceptPacketTruncationStats {
        core_members_omitted,
        supporting_members_omitted,
        likely_tests_omitted,
        evidence_omitted,
        relations_omitted: relation_truncation.relations_omitted,
        relation_evidence_omitted: relation_truncation.relation_evidence_omitted,
    }
    .into_view();
    let fallback = crate::concept_followthrough::ConceptFollowthroughTargets::default();
    let fallback_primary =
        if core_members.is_empty() && supporting_members.is_empty() && likely_tests.is_empty() {
            compact_optional_handle_for_id(session, prism, fallback.inspect_first.as_ref())?
        } else {
            None
        };
    let primary_handle = core_members
        .first()
        .map(|member| member.handle.clone())
        .or_else(|| {
            fallback_primary
                .as_ref()
                .map(|member| member.handle.clone())
        });
    let suggested_actions = compact_concept_suggested_actions(primary_handle.as_deref(), packet);
    let next_action = if truncation.is_some() {
        "This concept packet was trimmed for context. Open the strongest member now, or retry with `verbosity`: `full` if you need the complete packet.".to_string()
    } else if fallback_primary.is_some() {
        "Open the suggested follow-through target now, or decode the concept with a lens while the curated member bindings are refreshed.".to_string()
    } else {
        "Open the strongest core member or decode the concept with a lens.".to_string()
    };
    Ok(AgentConceptPacketView {
        handle: packet.handle.clone(),
        canonical_name: packet.canonical_name.clone(),
        summary: packet.summary.clone(),
        aliases: packet.aliases.clone(),
        confidence: packet.confidence,
        core_members,
        supporting_members,
        likely_tests,
        evidence,
        risk_hint: packet.risk_hint.clone(),
        decode_lenses: packet
            .decode_lenses
            .iter()
            .copied()
            .map(concept_decode_lens_view)
            .collect(),
        verbosity_applied: concept_verbosity_view(verbosity),
        truncation,
        scope: match packet.scope {
            ConceptScope::Local => ConceptScopeView::Local,
            ConceptScope::Session => ConceptScopeView::Session,
            ConceptScope::Repo => ConceptScopeView::Repo,
        },
        provenance: ConceptProvenanceView {
            origin: packet.provenance.origin.clone(),
            kind: packet.provenance.kind.clone(),
            task_id: packet.provenance.task_id.clone(),
        },
        publication: packet
            .publication
            .clone()
            .map(|publication| ConceptPublicationView {
                published_at: publication.published_at,
                last_reviewed_at: publication.last_reviewed_at,
                status: match publication.status {
                    ConceptPublicationStatus::Active => ConceptPublicationStatusView::Active,
                    ConceptPublicationStatus::Retired => ConceptPublicationStatusView::Retired,
                },
                supersedes: publication.supersedes,
                retired_at: publication.retired_at,
                retirement_reason: publication.retirement_reason,
            }),
        relations,
        resolution: resolution.map(|resolution| ConceptResolutionView {
            score: resolution.score,
            reasons: resolution.reasons,
        }),
        binding_metadata: include_binding_metadata.then(|| ConceptBindingMetadataView {
            core_member_lineages: packet
                .core_member_lineages
                .iter()
                .cloned()
                .map(|lineage| lineage.map(|lineage| lineage.0.to_string()))
                .collect(),
            supporting_member_lineages: packet
                .supporting_member_lineages
                .iter()
                .cloned()
                .map(|lineage| lineage.map(|lineage| lineage.0.to_string()))
                .collect(),
            likely_test_lineages: packet
                .likely_test_lineages
                .iter()
                .cloned()
                .map(|lineage| lineage.map(|lineage| lineage.0.to_string()))
                .collect(),
        }),
        next_action: Some(next_action),
        suggested_actions,
    })
}

fn resolve_concept_alternates(
    prism: &Prism,
    session: &SessionState,
    args: &PrismConceptArgs,
    selected_handle: &str,
    verbosity: ConceptVerbosity,
    include_binding_metadata: bool,
) -> Result<Vec<AgentConceptPacketView>> {
    let Some(query) = args.query.as_deref() else {
        return Ok(Vec::new());
    };
    let resolutions =
        resolve_concepts_for_task_context(prism, session, args.task_id.as_deref(), query, 3);
    if !concept_resolution_is_ambiguous(&resolutions) {
        return Ok(Vec::new());
    }
    resolutions
        .into_iter()
        .filter(|resolution| resolution.packet.handle != selected_handle)
        .map(|resolution| {
            let packet = resolution.packet.clone();
            agent_concept_packet_view(
                session,
                prism,
                &packet,
                verbosity,
                Some(resolution),
                include_binding_metadata,
            )
        })
        .collect()
}

pub(super) fn compact_handles_for_ids(
    session: &SessionState,
    prism: &Prism,
    ids: &[NodeId],
) -> Result<Vec<AgentTargetHandleView>> {
    let symbols = symbol_views_for_ids(prism, ids.to_vec())?;
    Ok(symbols
        .iter()
        .map(|symbol| compact_target_view(session, symbol, None, None))
        .collect())
}

fn compact_optional_handle_for_id(
    session: &SessionState,
    prism: &Prism,
    id: Option<&NodeId>,
) -> Result<Option<AgentTargetHandleView>> {
    let Some(id) = id else {
        return Ok(None);
    };
    Ok(
        compact_handles_for_ids(session, prism, std::slice::from_ref(id))?
            .into_iter()
            .next(),
    )
}

pub(super) fn compact_concept_workset_result(
    session: &SessionState,
    prism: &Prism,
    handle: &str,
) -> Result<AgentWorksetResultView> {
    let selection = compact_concept_selection(session, prism, handle)?;
    let suggested_actions =
        compact_concept_member_followups(&selection.packet, &selection.primary.handle);
    budgeted_workset_result_with_followups(
        selection.primary.clone(),
        selection.supporting_reads.clone(),
        selection.likely_tests,
        selection.packet.summary.clone(),
        false,
        Some(compact_concept_workset_next_action(
            &selection.packet,
            &selection.primary,
            selection.supporting_reads.first(),
        )),
        suggested_actions,
    )
}

pub(super) fn compact_concept_selection(
    session: &SessionState,
    prism: &Prism,
    handle: &str,
) -> Result<CompactConceptSelection> {
    let packet = prism
        .concept_by_handle(handle)
        .ok_or_else(|| anyhow!("no concept packet matched `{handle}`"))?;
    let fallback = concept_followthrough_targets(prism, &packet);
    let mut core_members = compact_handles_for_ids(session, prism, &packet.core_members)?;
    let mut supporting_reads = core_members.split_off(core_members.len().min(1));
    supporting_reads.extend(compact_handles_for_ids(
        session,
        prism,
        &packet.supporting_members,
    )?);
    dedupe_handle_views(&mut supporting_reads);
    let mut likely_tests = compact_handles_for_ids(session, prism, &packet.likely_tests)?;
    if supporting_reads.is_empty() {
        supporting_reads.extend(compact_handles_for_ids(
            session,
            prism,
            &fallback.supporting_reads,
        )?);
        dedupe_handle_views(&mut supporting_reads);
    }
    if likely_tests.is_empty() {
        likely_tests.extend(compact_handles_for_ids(
            session,
            prism,
            &fallback.likely_tests,
        )?);
        dedupe_handle_views(&mut likely_tests);
    }
    let fallback_primary =
        compact_optional_handle_for_id(session, prism, fallback.inspect_first.as_ref())?;
    let has_explicit_members = !packet.core_members.is_empty()
        || !packet.supporting_members.is_empty()
        || !packet.likely_tests.is_empty();
    let primary = if has_explicit_members {
        core_members
            .into_iter()
            .next()
            .or_else(|| supporting_reads.first().cloned())
            .or_else(|| likely_tests.first().cloned())
            .or(fallback_primary)
    } else {
        fallback_primary
            .clone()
            .or_else(|| supporting_reads.first().cloned())
            .or_else(|| likely_tests.first().cloned())
    }
    .ok_or_else(|| anyhow!("concept `{}` has no reusable members", packet.handle))?;
    supporting_reads.retain(|candidate| candidate.handle != primary.handle);
    likely_tests.retain(|candidate| candidate.handle != primary.handle);
    Ok(CompactConceptSelection {
        packet,
        primary,
        supporting_reads,
        likely_tests,
    })
}

fn compact_concept_suggested_actions(
    primary_handle: Option<&str>,
    packet: &ConceptPacket,
) -> Vec<AgentSuggestedActionView> {
    let Some(primary_handle) = primary_handle else {
        return Vec::new();
    };
    let mut actions = vec![
        suggested_open_action(primary_handle, prism_js::AgentOpenMode::Focus),
        suggested_workset_action(primary_handle),
    ];
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Validation))
    {
        actions.push(suggested_expand_action(
            primary_handle,
            prism_js::AgentExpandKind::Validation,
        ));
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Timeline))
    {
        actions.push(suggested_expand_action(
            primary_handle,
            prism_js::AgentExpandKind::Timeline,
        ));
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Memory))
    {
        actions.push(suggested_expand_action(
            primary_handle,
            prism_js::AgentExpandKind::Memory,
        ));
    }
    dedupe_suggested_actions(actions)
}

fn compact_concept_member_followups(
    packet: &ConceptPacket,
    primary_handle: &str,
) -> Vec<AgentSuggestedActionView> {
    let mut actions = vec![suggested_open_action(
        primary_handle,
        prism_js::AgentOpenMode::Focus,
    )];
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Validation))
    {
        actions.push(suggested_expand_action(
            primary_handle,
            prism_js::AgentExpandKind::Validation,
        ));
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Timeline))
    {
        actions.push(suggested_expand_action(
            primary_handle,
            prism_js::AgentExpandKind::Timeline,
        ));
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Memory))
    {
        actions.push(suggested_expand_action(
            primary_handle,
            prism_js::AgentExpandKind::Memory,
        ));
    }
    dedupe_suggested_actions(actions)
}

fn compact_concept_workset_next_action(
    packet: &ConceptPacket,
    primary: &AgentTargetHandleView,
    first_supporting_read: Option<&AgentTargetHandleView>,
) -> String {
    if matches!(
        primary.kind,
        prism_ir::NodeKind::MarkdownHeading | prism_ir::NodeKind::Document
    ) {
        if let Some(next_owner) = first_supporting_read {
            return format!(
                "Use prism_open on the doc or spec section first, then open `{}` to continue into the code owner path.",
                next_owner.path
            );
        }
    }
    if packet
        .decode_lenses
        .iter()
        .any(|lens| matches!(lens, ConceptDecodeLens::Validation))
    {
        "Use prism_open on a core member, or prism_concept with `handle` and `lens`: `validation` for the broader concept view.".to_string()
    } else {
        "Use prism_open on a core member, or prism_concept with a decode lens for broader concept context.".to_string()
    }
}

fn dedupe_handle_views(handles: &mut Vec<AgentTargetHandleView>) {
    let mut seen = HashSet::<String>::new();
    handles.retain(|handle| seen.insert(handle.handle.clone()));
}

fn decode_concept(
    session: &SessionState,
    prism: &Prism,
    packet: &ConceptPacket,
    lens: &crate::PrismConceptLensInput,
    verbosity: ConceptVerbosity,
) -> Result<ConceptDecodeView> {
    let concept = concept_packet_view(prism, packet.clone(), verbosity, false, None);
    let members = symbol_views_for_ids(prism, packet.core_members.clone())?;
    let primary = members.first().cloned();
    let supporting_reads = symbol_views_for_ids(prism, packet.supporting_members.clone())?;
    let likely_tests = symbol_views_for_ids(prism, packet.likely_tests.clone())?;
    let anchors = prism.anchors_for(
        &packet
            .core_members
            .iter()
            .cloned()
            .map(AnchorRef::Node)
            .collect::<Vec<_>>(),
    );
    let recent_failures = prism.query_outcomes(&OutcomeRecallQuery {
        anchors: anchors.clone(),
        kinds: Some(vec![OutcomeKind::FailureObserved]),
        limit: CONCEPT_FAILURE_LIMIT,
        ..OutcomeRecallQuery::default()
    });
    let related_memory = session
        .notes
        .recall(&RecallQuery {
            focus: anchors,
            limit: CONCEPT_MEMORY_LIMIT,
            ..RecallQuery::default()
        })?
        .into_iter()
        .map(scored_memory_view)
        .collect::<Vec<_>>();
    let recent_patches = concept_recent_patches(prism, &packet.core_members)?;
    let validation_recipe = packet
        .core_members
        .first()
        .map(|primary_id| validation_recipe_view_with(prism, session, primary_id));
    Ok(ConceptDecodeView {
        concept,
        lens: concept_decode_lens_view(match lens {
            crate::PrismConceptLensInput::Open => ConceptDecodeLens::Open,
            crate::PrismConceptLensInput::Workset => ConceptDecodeLens::Workset,
            crate::PrismConceptLensInput::Validation => ConceptDecodeLens::Validation,
            crate::PrismConceptLensInput::Timeline => ConceptDecodeLens::Timeline,
            crate::PrismConceptLensInput::Memory => ConceptDecodeLens::Memory,
        }),
        primary,
        members,
        supporting_reads,
        likely_tests,
        recent_failures,
        related_memory,
        recent_patches,
        validation_recipe,
        evidence: packet.evidence.clone(),
    })
}

fn concept_verbosity(input: Option<crate::PrismConceptVerbosityInput>) -> ConceptVerbosity {
    match input.unwrap_or(crate::PrismConceptVerbosityInput::Summary) {
        crate::PrismConceptVerbosityInput::Summary => ConceptVerbosity::Summary,
        crate::PrismConceptVerbosityInput::Standard => ConceptVerbosity::Standard,
        crate::PrismConceptVerbosityInput::Full => ConceptVerbosity::Full,
    }
}

fn concept_recent_patches(
    prism: &Prism,
    members: &[NodeId],
) -> Result<Vec<prism_js::PatchEventView>> {
    let mut patches = Vec::new();
    let mut seen = HashSet::<String>::new();
    for member in members {
        for patch in recent_patches(prism, Some(member), None, None, None, CONCEPT_PATCH_LIMIT)? {
            if seen.insert(patch.event_id.clone()) {
                patches.push(patch);
            }
            if patches.len() >= CONCEPT_PATCH_LIMIT {
                return Ok(patches);
            }
        }
    }
    Ok(patches)
}
