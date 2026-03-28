use std::collections::HashSet;

use prism_ir::AnchorRef;
use prism_js::{
    AgentConceptPacketView, AgentConceptResultView, AgentSuggestedActionView,
    ConceptBindingMetadataView, ConceptDecodeView, ConceptProvenanceView,
    ConceptPublicationStatusView, ConceptPublicationView, ConceptScopeView,
};
use prism_memory::{MemoryModule, OutcomeKind, OutcomeRecallQuery, RecallQuery};
use prism_query::{ConceptDecodeLens, ConceptPacket, ConceptPublicationStatus, ConceptScope};

use super::suggested_actions::{
    dedupe_suggested_actions, suggested_expand_action, suggested_open_action,
    suggested_workset_action,
};
use super::*;
use crate::{
    concept_decode_lens_view, concept_packet_view, recent_patches, scored_memory_view,
    symbol_views_for_ids, validation_recipe_view_with,
};

const CONCEPT_PATCH_LIMIT: usize = 4;
const CONCEPT_MEMORY_LIMIT: usize = 4;
const CONCEPT_FAILURE_LIMIT: usize = 8;

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
            move |host, _| {
                let prism = host.current_prism();
                let packet = resolve_concept_packet(prism.as_ref(), &args)?;
                let packet_view = agent_concept_packet_view(
                    session.as_ref(),
                    prism.as_ref(),
                    &packet,
                    args.include_binding_metadata.unwrap_or(false),
                )?;
                let decode = args
                    .lens
                    .as_ref()
                    .map(|lens| decode_concept(session.as_ref(), prism.as_ref(), &packet, lens))
                    .transpose()?;
                Ok((
                    AgentConceptResultView {
                        packet: packet_view,
                        decode,
                    },
                    Vec::new(),
                ))
            },
        )
    }
}

fn resolve_concept_packet(prism: &Prism, args: &PrismConceptArgs) -> Result<ConceptPacket> {
    match (args.handle.as_deref(), args.query.as_deref()) {
        (Some(handle), _) => prism
            .concept_by_handle(handle)
            .ok_or_else(|| anyhow!("no concept packet matched `{handle}`")),
        (None, Some(query)) => prism
            .concept(query)
            .ok_or_else(|| anyhow!("no concept packet matched `{query}`")),
        (None, None) => Err(anyhow!("prism_concept requires `handle` or `query`")),
    }
}

fn agent_concept_packet_view(
    session: &SessionState,
    prism: &Prism,
    packet: &ConceptPacket,
    include_binding_metadata: bool,
) -> Result<AgentConceptPacketView> {
    let core_members = compact_handles_for_ids(session, prism, &packet.core_members)?;
    let supporting_members = compact_handles_for_ids(session, prism, &packet.supporting_members)?;
    let likely_tests = compact_handles_for_ids(session, prism, &packet.likely_tests)?;
    let primary_handle = core_members.first().map(|member| member.handle.clone());
    let suggested_actions = compact_concept_suggested_actions(primary_handle.as_deref(), packet);
    Ok(AgentConceptPacketView {
        handle: packet.handle.clone(),
        canonical_name: packet.canonical_name.clone(),
        summary: packet.summary.clone(),
        aliases: packet.aliases.clone(),
        confidence: packet.confidence,
        core_members,
        supporting_members,
        likely_tests,
        evidence: packet.evidence.clone(),
        risk_hint: packet.risk_hint.clone(),
        decode_lenses: packet
            .decode_lenses
            .iter()
            .copied()
            .map(concept_decode_lens_view)
            .collect(),
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
        next_action: Some(
            "Open the strongest core member or decode the concept with a lens.".to_string(),
        ),
        suggested_actions,
    })
}

fn compact_handles_for_ids(
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

fn decode_concept(
    session: &SessionState,
    prism: &Prism,
    packet: &ConceptPacket,
    lens: &crate::PrismConceptLensInput,
) -> Result<ConceptDecodeView> {
    let concept = concept_packet_view(packet.clone(), false);
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
