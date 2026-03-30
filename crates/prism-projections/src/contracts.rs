use std::collections::HashMap;

use crate::types::{
    ContractEvent, ContractEventAction, ContractGuarantee, ContractPacket, ContractProvenance,
    ContractPublication, ContractPublicationStatus, ContractResolution, ContractScope,
    ContractStatus,
};

pub(crate) fn resolve_contracts(
    contracts: &[ContractPacket],
    query: &str,
    limit: usize,
) -> Vec<ContractResolution> {
    let normalized_query = normalize_text(query);
    let query_tokens = normalize_tokens(query);
    if normalized_query.is_empty() || query_tokens.is_empty() {
        return Vec::new();
    }
    let query_trigrams = trigrams(&normalized_query);
    let mut ranked = contracts
        .iter()
        .filter_map(|contract| {
            contract_query_resolution(contract, &normalized_query, &query_tokens, &query_trigrams)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.packet.handle.cmp(&right.packet.handle))
    });
    if limit > 0 {
        ranked.truncate(limit);
    }
    ranked
}

pub(crate) fn contract_by_handle(
    contracts: &[ContractPacket],
    handle: &str,
) -> Option<ContractPacket> {
    let normalized = normalize_handle(handle);
    contracts
        .iter()
        .find(|contract| normalize_handle(&contract.handle) == normalized)
        .cloned()
}

pub(crate) fn merge_contract_packets(curated: &[ContractPacket]) -> Vec<ContractPacket> {
    let mut merged = HashMap::<String, ContractPacket>::new();
    for contract in curated {
        merged.insert(normalize_handle(&contract.handle), contract.clone());
    }
    let mut packets = merged.into_values().collect::<Vec<_>>();
    packets.sort_by(|left, right| left.handle.cmp(&right.handle));
    packets
}

pub fn curated_contracts_from_events(events: &[ContractEvent]) -> Vec<ContractPacket> {
    let mut contracts = HashMap::<String, ContractPacket>::new();
    for event in events {
        let key = normalize_handle(&event.contract.handle);
        let previous = contracts.get(&key).cloned();
        let contract = contract_from_event(previous.as_ref(), event);
        if contract.status == ContractStatus::Retired {
            contracts.remove(&key);
        } else {
            contracts.insert(key, contract);
        }
    }
    let mut contracts = contracts.into_values().collect::<Vec<_>>();
    contracts.sort_by(|left, right| left.handle.cmp(&right.handle));
    contracts
}

pub fn contract_from_event(
    previous: Option<&ContractPacket>,
    event: &ContractEvent,
) -> ContractPacket {
    let contract = contract_event_post_image(previous, event);
    normalize_curated_contract(event, previous, contract)
}

pub fn canonical_contract_handle(name: &str) -> String {
    let slug = normalize_slug(name);
    if slug.is_empty() {
        "contract://contract".to_string()
    } else {
        format!("contract://{slug}")
    }
}

fn normalize_curated_contract(
    event: &ContractEvent,
    previous: Option<&ContractPacket>,
    mut contract: ContractPacket,
) -> ContractPacket {
    if contract.provenance == ContractProvenance::default() {
        contract.provenance = ContractProvenance {
            origin: "repo_mutation".to_string(),
            kind: match event.action {
                ContractEventAction::Promote => "manual_contract_promote".to_string(),
                ContractEventAction::Update => "manual_contract_update".to_string(),
                ContractEventAction::Retire => "manual_contract_retire".to_string(),
                ContractEventAction::AttachEvidence => {
                    "manual_contract_attach_evidence".to_string()
                }
                ContractEventAction::AttachValidation => {
                    "manual_contract_attach_validation".to_string()
                }
                ContractEventAction::RecordConsumer => {
                    "manual_contract_record_consumer".to_string()
                }
                ContractEventAction::SetStatus => "manual_contract_set_status".to_string(),
            },
            task_id: event.task_id.clone(),
        };
    } else if contract.provenance.task_id.is_none() {
        contract.provenance.task_id = event.task_id.clone();
    }

    if matches!(event.action, ContractEventAction::Retire) {
        contract.status = ContractStatus::Retired;
    } else if contract.scope == ContractScope::Repo && contract.status == ContractStatus::Candidate
    {
        contract.status = ContractStatus::Active;
    }

    contract.guarantees = normalize_contract_guarantees(contract.guarantees);

    if contract.scope == ContractScope::Repo || contract.status == ContractStatus::Retired {
        let mut publication = contract
            .publication
            .clone()
            .unwrap_or_else(|| previous_publication(previous, event.recorded_at));
        if publication.published_at == 0 && contract.scope == ContractScope::Repo {
            publication.published_at = previous
                .and_then(|packet| packet.publication.as_ref())
                .map(|value| value.published_at)
                .filter(|value| *value > 0)
                .unwrap_or(event.recorded_at);
        }
        publication.last_reviewed_at = Some(event.recorded_at);
        if contract.status == ContractStatus::Retired {
            publication.status = ContractPublicationStatus::Retired;
            publication.retired_at = Some(event.recorded_at);
            publication.retirement_reason = event
                .patch
                .as_ref()
                .and_then(|patch| patch.retirement_reason.clone())
                .or_else(|| publication.retirement_reason.clone())
                .or_else(|| Some("retired".to_string()));
        } else {
            publication.status = ContractPublicationStatus::Active;
            publication.retired_at = None;
            publication.retirement_reason = None;
        }
        contract.publication = Some(publication);
    } else {
        contract.publication = None;
    }
    contract
}

fn normalize_contract_guarantees(guarantees: Vec<ContractGuarantee>) -> Vec<ContractGuarantee> {
    let mut seen = HashMap::<String, usize>::new();
    guarantees
        .into_iter()
        .map(|mut guarantee| {
            let base = normalize_guarantee_slug(if guarantee.id.trim().is_empty() {
                &guarantee.statement
            } else {
                &guarantee.id
            });
            let counter = seen.entry(base.clone()).or_insert(0);
            *counter += 1;
            guarantee.id = if *counter == 1 {
                base
            } else {
                format!("{base}_{}", *counter)
            };
            guarantee
        })
        .collect()
}

fn contract_event_post_image(
    previous: Option<&ContractPacket>,
    event: &ContractEvent,
) -> ContractPacket {
    let Some(previous) = previous else {
        return event.contract.clone();
    };
    let Some(patch) = event.patch.as_ref() else {
        return event.contract.clone();
    };

    let mut contract = previous.clone();
    if has_patch_field(&patch.set_fields, "kind") {
        contract.kind = patch.kind.unwrap_or(event.contract.kind);
    }
    if has_patch_field(&patch.set_fields, "name") {
        contract.name = patch
            .name
            .clone()
            .unwrap_or_else(|| event.contract.name.clone());
    }
    if has_patch_field(&patch.set_fields, "summary") {
        contract.summary = patch
            .summary
            .clone()
            .unwrap_or_else(|| event.contract.summary.clone());
    }
    if has_patch_field(&patch.set_fields, "aliases") {
        contract.aliases = patch
            .aliases
            .clone()
            .unwrap_or_else(|| event.contract.aliases.clone());
    }
    if has_patch_field(&patch.set_fields, "subject") {
        contract.subject = patch
            .subject
            .clone()
            .unwrap_or_else(|| event.contract.subject.clone());
    }
    if has_patch_field(&patch.set_fields, "guarantees") {
        contract.guarantees = patch
            .guarantees
            .clone()
            .unwrap_or_else(|| event.contract.guarantees.clone());
    }
    if has_patch_field(&patch.set_fields, "assumptions") {
        contract.assumptions = patch
            .assumptions
            .clone()
            .unwrap_or_else(|| event.contract.assumptions.clone());
    }
    if has_patch_field(&patch.set_fields, "consumers") {
        contract.consumers = patch
            .consumers
            .clone()
            .unwrap_or_else(|| event.contract.consumers.clone());
    }
    if has_patch_field(&patch.set_fields, "validations") {
        contract.validations = patch
            .validations
            .clone()
            .unwrap_or_else(|| event.contract.validations.clone());
    }
    if has_patch_field(&patch.set_fields, "stability") {
        contract.stability = patch.stability.unwrap_or(event.contract.stability);
    }
    if has_patch_field(&patch.set_fields, "compatibility") {
        contract.compatibility = patch
            .compatibility
            .clone()
            .unwrap_or_else(|| event.contract.compatibility.clone());
    }
    if has_patch_field(&patch.set_fields, "evidence") {
        contract.evidence = patch
            .evidence
            .clone()
            .unwrap_or_else(|| event.contract.evidence.clone());
    }
    if has_patch_field(&patch.set_fields, "status") {
        contract.status = patch.status.unwrap_or(event.contract.status);
    }
    if has_patch_field(&patch.set_fields, "scope") {
        contract.scope = patch.scope.unwrap_or(event.contract.scope);
    }
    if has_patch_field(&patch.set_fields, "supersedes") {
        let publication = contract
            .publication
            .get_or_insert_with(|| event.contract.publication.clone().unwrap_or_default());
        publication.supersedes = patch.supersedes.clone().unwrap_or_else(|| {
            event
                .contract
                .publication
                .as_ref()
                .map(|publication| publication.supersedes.clone())
                .unwrap_or_default()
        });
    }

    contract
}

fn has_patch_field(fields: &[String], target: &str) -> bool {
    fields.iter().any(|field| field == target)
}

fn previous_publication(
    previous: Option<&ContractPacket>,
    recorded_at: u64,
) -> ContractPublication {
    let mut publication = previous
        .and_then(|packet| packet.publication.clone())
        .unwrap_or_default();
    if publication.published_at == 0 {
        publication.published_at = recorded_at;
    }
    publication
}

fn contract_query_resolution(
    contract: &ContractPacket,
    normalized_query: &str,
    query_tokens: &[String],
    query_trigrams: &[String],
) -> Option<ContractResolution> {
    let mut score = 0;
    let mut reasons = Vec::new();

    let handle = normalize_handle(&contract.handle);
    if handle == *normalized_query {
        score += 180;
        reasons.push("handle exact match".to_string());
    }

    let normalized_name = normalize_text(&contract.name);
    if normalized_name == *normalized_query {
        score += 160;
        reasons.push("name exact match".to_string());
    } else if normalized_name.contains(normalized_query) {
        score += 90;
        reasons.push("name term match".to_string());
    }

    let normalized_aliases = contract
        .aliases
        .iter()
        .map(|alias| normalize_text(alias))
        .collect::<Vec<_>>();
    if normalized_aliases
        .iter()
        .any(|alias| alias == normalized_query)
    {
        score += 120;
        reasons.push("alias exact match".to_string());
    } else if normalized_aliases
        .iter()
        .any(|alias| alias.contains(normalized_query))
    {
        score += 70;
        reasons.push("alias term match".to_string());
    }

    let normalized_summary = normalize_text(&contract.summary);
    if normalized_summary.contains(normalized_query) {
        score += 45;
        reasons.push("summary term match".to_string());
    }

    let normalized_guarantees = contract
        .guarantees
        .iter()
        .map(|guarantee| normalize_text(&guarantee.statement))
        .collect::<Vec<_>>();
    if normalized_guarantees
        .iter()
        .any(|statement| statement.contains(normalized_query))
    {
        score += 55;
        reasons.push("guarantee term match".to_string());
    }

    let typo_bonus = typo_bonus(
        &normalized_name,
        &normalized_aliases,
        &normalized_summary,
        query_trigrams,
    );
    if let Some((bonus, reason)) = typo_bonus {
        score += bonus;
        reasons.push(reason);
    }

    let matched_terms = query_tokens
        .iter()
        .filter(|token| {
            handle.contains(*token)
                || normalized_name.contains(*token)
                || normalized_summary.contains(*token)
                || normalized_aliases
                    .iter()
                    .any(|alias| alias.contains(*token))
                || normalized_guarantees
                    .iter()
                    .any(|statement| statement.contains(*token))
        })
        .count();
    if matched_terms == 0 {
        return None;
    }
    score += (matched_terms as i32) * 12;
    reasons.push(format!(
        "matched {matched_terms}/{} significant query terms",
        query_tokens.len()
    ));

    Some(ContractResolution {
        packet: contract.clone(),
        score,
        reasons,
    })
}

fn typo_bonus(
    normalized_name: &str,
    normalized_aliases: &[String],
    normalized_summary: &str,
    query_trigrams: &[String],
) -> Option<(i32, String)> {
    if query_trigrams.is_empty() {
        return None;
    }
    let mut candidates = Vec::with_capacity(2 + normalized_aliases.len());
    candidates.push((normalized_name, "fuzzy name"));
    candidates.push((normalized_summary, "fuzzy summary"));
    for alias in normalized_aliases {
        candidates.push((alias.as_str(), "fuzzy alias"));
    }
    candidates
        .into_iter()
        .filter_map(|(candidate, label)| {
            let overlap = trigram_overlap(candidate, query_trigrams);
            if overlap >= 0.45 {
                Some((((overlap * 60.0) as i32), label.to_string()))
            } else {
                None
            }
        })
        .max_by_key(|(bonus, _)| *bonus)
}

fn trigram_overlap(candidate: &str, query_trigrams: &[String]) -> f32 {
    let candidate_trigrams = trigrams(candidate);
    if candidate_trigrams.is_empty() || query_trigrams.is_empty() {
        return 0.0;
    }
    let overlap = query_trigrams
        .iter()
        .filter(|gram| candidate_trigrams.contains(*gram))
        .count();
    overlap as f32 / query_trigrams.len() as f32
}

fn normalize_handle(handle: &str) -> String {
    handle.trim().to_ascii_lowercase()
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_tokens(value: &str) -> Vec<String> {
    normalize_text(value)
        .split_whitespace()
        .map(|token| token.to_string())
        .collect()
}

fn normalize_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep && !slug.is_empty() {
            slug.push('_');
            last_was_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    slug
}

fn normalize_guarantee_slug(value: &str) -> String {
    let slug = normalize_slug(value);
    if slug.is_empty() {
        "guarantee".to_string()
    } else {
        slug
    }
}

fn trigrams(value: &str) -> Vec<String> {
    let normalized = normalize_text(value);
    if normalized.len() < 3 {
        return if normalized.is_empty() {
            Vec::new()
        } else {
            vec![normalized]
        };
    }
    let chars = normalized.chars().collect::<Vec<_>>();
    let mut grams = Vec::new();
    for window in chars.windows(3) {
        grams.push(window.iter().collect());
    }
    grams.sort();
    grams.dedup();
    grams
}
