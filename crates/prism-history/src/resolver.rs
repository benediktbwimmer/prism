use std::collections::{HashMap, HashSet};

use prism_ir::{
    ChangeTrigger, LineageEvent, LineageEventKind, LineageEvidence, LineageId, NodeId,
    ObservedChangeSet, ObservedNode, SymbolFingerprint,
};

use crate::store::HistoryStore;

const STRONG_MATCH_THRESHOLD: f32 = 0.7;
const AMBIGUITY_EPSILON: f32 = 0.05;
const SAME_CONTAINER_BONUS: f32 = 0.15;
const FILE_HINT_BONUS: f32 = 0.10;

#[derive(Debug, Clone)]
struct CandidateMatch {
    removed_index: usize,
    added_index: usize,
    kind: LineageEventKind,
    score: f32,
    evidence: Vec<LineageEvidence>,
}

#[derive(Debug, Clone)]
struct ResolutionGroup {
    kind: LineageEventKind,
    removed_indices: Vec<usize>,
    added_indices: Vec<usize>,
    confidence: f32,
    evidence: Vec<LineageEvidence>,
}

#[derive(Debug, Clone)]
struct ReviveCandidate {
    added_index: usize,
    lineage: LineageId,
    before: Vec<NodeId>,
    score: f32,
    evidence: Vec<LineageEvidence>,
}

pub(crate) fn resolve_change_set(
    store: &mut HistoryStore,
    change_set: &ObservedChangeSet,
) -> Vec<LineageEvent> {
    let mut emitted = Vec::new();

    for (before, after) in &change_set.updated {
        let lineage = store
            .node_to_lineage
            .get(&before.node.id)
            .cloned()
            .unwrap_or_else(|| store.alloc_lineage());
        store.tombstones.remove(&lineage);
        store.assign_node_lineage(after.node.id.clone(), lineage.clone());
        emitted.push(store.make_event(
            change_set,
            lineage,
            LineageEventKind::Updated,
            vec![before.node.id.clone()],
            vec![after.node.id.clone()],
            1.0,
            vec![LineageEvidence::ExactNodeId],
        ));
    }

    let candidates =
        match_lineage_candidates(store, change_set, &change_set.removed, &change_set.added);
    let by_removed = candidate_indexes_by_removed(change_set.removed.len(), &candidates);
    let by_added = candidate_indexes_by_added(change_set.added.len(), &candidates);
    let mut consumed_removed = HashSet::new();
    let mut consumed_added = HashSet::new();

    for removed_index in 0..change_set.removed.len() {
        if let Some(group) = split_group(
            removed_index,
            &candidates,
            &by_removed,
            &by_added,
            &consumed_removed,
            &consumed_added,
        ) {
            mark_group_consumed(&group, &mut consumed_removed, &mut consumed_added);
            apply_group(
                store,
                change_set,
                &change_set.removed,
                &change_set.added,
                group,
                &mut emitted,
            );
        }
    }

    for added_index in 0..change_set.added.len() {
        if let Some(group) = merge_group(
            added_index,
            &candidates,
            &by_removed,
            &by_added,
            &consumed_removed,
            &consumed_added,
        ) {
            mark_group_consumed(&group, &mut consumed_removed, &mut consumed_added);
            apply_group(
                store,
                change_set,
                &change_set.removed,
                &change_set.added,
                group,
                &mut emitted,
            );
        }
    }

    let mut candidate_order = (0..candidates.len()).collect::<Vec<_>>();
    candidate_order
        .sort_by(|left, right| compare_candidate(&candidates[*left], &candidates[*right]));
    for candidate_index in candidate_order {
        if let Some(group) = unique_group(
            candidate_index,
            &candidates,
            &by_removed,
            &by_added,
            &consumed_removed,
            &consumed_added,
        ) {
            mark_group_consumed(&group, &mut consumed_removed, &mut consumed_added);
            apply_group(
                store,
                change_set,
                &change_set.removed,
                &change_set.added,
                group,
                &mut emitted,
            );
        }
    }

    for added_index in 0..change_set.added.len() {
        if let Some(group) = ambiguous_group_for_added(
            added_index,
            &candidates,
            &by_added,
            &consumed_removed,
            &consumed_added,
        ) {
            mark_group_consumed(&group, &mut consumed_removed, &mut consumed_added);
            apply_group(
                store,
                change_set,
                &change_set.removed,
                &change_set.added,
                group,
                &mut emitted,
            );
        }
    }

    for removed_index in 0..change_set.removed.len() {
        if let Some(group) = ambiguous_group_for_removed(
            removed_index,
            &candidates,
            &by_removed,
            &consumed_removed,
            &consumed_added,
        ) {
            mark_group_consumed(&group, &mut consumed_removed, &mut consumed_added);
            apply_group(
                store,
                change_set,
                &change_set.removed,
                &change_set.added,
                group,
                &mut emitted,
            );
        }
    }

    for (index, removed) in change_set.removed.iter().enumerate() {
        if consumed_removed.contains(&index) {
            continue;
        }
        let lineage = store
            .remove_node_lineage(&removed.node.id)
            .unwrap_or_else(|| store.alloc_lineage());
        store.record_tombstone(&lineage, removed);
        emitted.push(store.make_event(
            change_set,
            lineage,
            LineageEventKind::Died,
            vec![removed.node.id.clone()],
            Vec::new(),
            1.0,
            Vec::new(),
        ));
    }

    let revive_candidates =
        revive_candidates(store, change_set, &change_set.added, &consumed_added);
    let revive_by_added = revive_indexes_by_added(change_set.added.len(), &revive_candidates);
    let mut revived_added = HashSet::new();

    for added_index in 0..change_set.added.len() {
        if consumed_added.contains(&added_index) || revived_added.contains(&added_index) {
            continue;
        }

        let Some(candidate_indexes) = revive_by_added.get(added_index) else {
            continue;
        };
        let Some(top_indexes) = top_revive_candidates(candidate_indexes, &revive_candidates) else {
            continue;
        };
        revived_added.insert(added_index);
        let added = &change_set.added[added_index];

        if top_indexes.len() == 1 {
            let candidate = &revive_candidates[top_indexes[0]];
            store.tombstones.remove(&candidate.lineage);
            store.assign_node_lineage(added.node.id.clone(), candidate.lineage.clone());
            emitted.push(store.make_event(
                change_set,
                candidate.lineage.clone(),
                LineageEventKind::Revived,
                candidate.before.clone(),
                vec![added.node.id.clone()],
                candidate.score,
                candidate.evidence.clone(),
            ));
            continue;
        }

        let candidate_group = top_indexes
            .iter()
            .map(|index| &revive_candidates[*index])
            .collect::<Vec<_>>();
        let canonical = choose_lineage(
            candidate_group
                .iter()
                .map(|candidate| candidate.lineage.clone())
                .collect::<Vec<_>>(),
        );
        store.tombstones.remove(&canonical);
        store.assign_node_lineage(added.node.id.clone(), canonical.clone());
        let before = dedupe_node_ids(
            candidate_group
                .iter()
                .flat_map(|candidate| candidate.before.clone())
                .collect::<Vec<_>>(),
        );
        emitted.push(
            store.make_event(
                change_set,
                canonical,
                LineageEventKind::Ambiguous,
                before,
                vec![added.node.id.clone()],
                average_revive_score(&candidate_group),
                merge_evidence(
                    candidate_group
                        .iter()
                        .flat_map(|candidate| candidate.evidence.clone()),
                ),
            ),
        );
    }

    for (index, added) in change_set.added.iter().enumerate() {
        if consumed_added.contains(&index) || revived_added.contains(&index) {
            continue;
        }
        let lineage = store.alloc_lineage();
        store.assign_node_lineage(added.node.id.clone(), lineage.clone());
        emitted.push(store.make_event(
            change_set,
            lineage,
            LineageEventKind::Born,
            Vec::new(),
            vec![added.node.id.clone()],
            1.0,
            Vec::new(),
        ));
    }

    store.record_co_changes(&emitted);
    store.events.extend(emitted.iter().cloned());
    emitted
}

fn match_lineage_candidates(
    store: &HistoryStore,
    change_set: &ObservedChangeSet,
    removed: &[ObservedNode],
    added: &[ObservedNode],
) -> Vec<CandidateMatch> {
    let path_lineages = path_lineage_index(&store.node_to_lineage);
    let mut candidates = Vec::new();

    for (removed_index, before) in removed.iter().enumerate() {
        for (added_index, after) in added.iter().enumerate() {
            if before.node.kind != after.node.kind {
                continue;
            }

            let Some((mut score, mut evidence)) =
                fingerprint_match(&before.fingerprint, &after.fingerprint)
            else {
                continue;
            };

            if shares_container_lineage(before, after, &path_lineages) {
                score = (score + SAME_CONTAINER_BONUS).min(1.0);
                evidence.push(LineageEvidence::SameContainerLineage);
            }
            if let Some(file_hint) = file_hint_evidence(change_set) {
                score = (score + FILE_HINT_BONUS).min(1.0);
                evidence.push(file_hint);
            }

            candidates.push(CandidateMatch {
                removed_index,
                added_index,
                kind: classify_change(change_set, before, after),
                score,
                evidence: dedupe_evidence(evidence),
            });
        }
    }

    candidates
}

fn revive_candidates(
    store: &HistoryStore,
    change_set: &ObservedChangeSet,
    added: &[ObservedNode],
    consumed_added: &HashSet<usize>,
) -> Vec<ReviveCandidate> {
    let path_lineages = path_lineage_index(&store.node_to_lineage);
    let mut candidates = Vec::new();

    for (added_index, after) in added.iter().enumerate() {
        if consumed_added.contains(&added_index) {
            continue;
        }

        for tombstone in store.tombstones.values() {
            let Some((mut score, mut evidence)) =
                fingerprint_match(&tombstone.fingerprint, &after.fingerprint)
            else {
                continue;
            };
            if tombstone
                .nodes
                .first()
                .is_some_and(|before| before.kind != after.node.kind)
            {
                continue;
            }

            if tombstone
                .nodes
                .first()
                .and_then(|before| container_lineage_for_node(before, &path_lineages))
                .zip(container_lineage_for_node(&after.node.id, &path_lineages))
                .is_some_and(|(before, after)| before == after)
            {
                score = (score + SAME_CONTAINER_BONUS).min(1.0);
                evidence.push(LineageEvidence::SameContainerLineage);
            }
            if let Some(file_hint) = file_hint_evidence(change_set) {
                score = (score + FILE_HINT_BONUS).min(1.0);
                evidence.push(file_hint);
            }
            if score < STRONG_MATCH_THRESHOLD {
                continue;
            }

            candidates.push(ReviveCandidate {
                added_index,
                lineage: tombstone.lineage.clone(),
                before: tombstone.nodes.clone(),
                score,
                evidence: dedupe_evidence(evidence),
            });
        }
    }

    candidates.sort_by(compare_revive_candidate);
    candidates
}

fn apply_group(
    store: &mut HistoryStore,
    change_set: &ObservedChangeSet,
    removed: &[ObservedNode],
    added: &[ObservedNode],
    group: ResolutionGroup,
    emitted: &mut Vec<LineageEvent>,
) {
    let mut lineage_entries = group
        .removed_indices
        .iter()
        .map(|index| {
            let node = &removed[*index];
            let lineage = store
                .remove_node_lineage(&node.node.id)
                .unwrap_or_else(|| store.alloc_lineage());
            (*index, lineage)
        })
        .collect::<Vec<_>>();
    lineage_entries.sort_by(|left, right| left.1 .0.cmp(&right.1 .0));
    let canonical = lineage_entries
        .first()
        .map(|(_, lineage)| lineage.clone())
        .unwrap_or_else(|| store.alloc_lineage());
    store.tombstones.remove(&canonical);

    let before = group
        .removed_indices
        .iter()
        .map(|index| removed[*index].node.id.clone())
        .collect::<Vec<_>>();
    let after = group
        .added_indices
        .iter()
        .map(|index| {
            let node_id = added[*index].node.id.clone();
            store.assign_node_lineage(node_id.clone(), canonical.clone());
            node_id
        })
        .collect::<Vec<_>>();

    emitted.push(store.make_event(
        change_set,
        canonical.clone(),
        group.kind,
        before,
        after,
        group.confidence,
        group.evidence,
    ));

    for (removed_index, lineage) in lineage_entries.into_iter().skip(1) {
        store.record_tombstone(&lineage, &removed[removed_index]);
        emitted.push(store.make_event(
            change_set,
            lineage,
            LineageEventKind::Died,
            vec![removed[removed_index].node.id.clone()],
            Vec::new(),
            1.0,
            Vec::new(),
        ));
    }
}

fn compare_candidate(left: &CandidateMatch, right: &CandidateMatch) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.removed_index.cmp(&right.removed_index))
        .then_with(|| left.added_index.cmp(&right.added_index))
}

fn compare_revive_candidate(left: &ReviveCandidate, right: &ReviveCandidate) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.added_index.cmp(&right.added_index))
        .then_with(|| left.lineage.0.cmp(&right.lineage.0))
}

fn candidate_indexes_by_removed(count: usize, candidates: &[CandidateMatch]) -> Vec<Vec<usize>> {
    let mut groups = vec![Vec::new(); count];
    for (index, candidate) in candidates.iter().enumerate() {
        groups[candidate.removed_index].push(index);
    }
    for indexes in &mut groups {
        indexes.sort_by(|left, right| compare_candidate(&candidates[*left], &candidates[*right]));
    }
    groups
}

fn candidate_indexes_by_added(count: usize, candidates: &[CandidateMatch]) -> Vec<Vec<usize>> {
    let mut groups = vec![Vec::new(); count];
    for (index, candidate) in candidates.iter().enumerate() {
        groups[candidate.added_index].push(index);
    }
    for indexes in &mut groups {
        indexes.sort_by(|left, right| compare_candidate(&candidates[*left], &candidates[*right]));
    }
    groups
}

fn revive_indexes_by_added(count: usize, candidates: &[ReviveCandidate]) -> Vec<Vec<usize>> {
    let mut groups = vec![Vec::new(); count];
    for (index, candidate) in candidates.iter().enumerate() {
        groups[candidate.added_index].push(index);
    }
    groups
}

fn available_candidate_indexes(
    indexes: &[usize],
    candidates: &[CandidateMatch],
    consumed_removed: &HashSet<usize>,
    consumed_added: &HashSet<usize>,
) -> Vec<usize> {
    indexes
        .iter()
        .copied()
        .filter(|index| {
            let candidate = &candidates[*index];
            !consumed_removed.contains(&candidate.removed_index)
                && !consumed_added.contains(&candidate.added_index)
        })
        .collect()
}

fn top_candidate_indexes(indexes: &[usize], candidates: &[CandidateMatch]) -> Option<Vec<usize>> {
    let best = indexes.first().copied()?;
    let best_score = candidates[best].score;
    if best_score < STRONG_MATCH_THRESHOLD {
        return None;
    }
    Some(
        indexes
            .iter()
            .copied()
            .take_while(|index| (best_score - candidates[*index].score) <= AMBIGUITY_EPSILON)
            .collect(),
    )
}

fn top_revive_candidates(indexes: &[usize], candidates: &[ReviveCandidate]) -> Option<Vec<usize>> {
    let best = indexes.first().copied()?;
    let best_score = candidates[best].score;
    Some(
        indexes
            .iter()
            .copied()
            .take_while(|index| (best_score - candidates[*index].score) <= AMBIGUITY_EPSILON)
            .collect(),
    )
}

fn split_group(
    removed_index: usize,
    candidates: &[CandidateMatch],
    by_removed: &[Vec<usize>],
    by_added: &[Vec<usize>],
    consumed_removed: &HashSet<usize>,
    consumed_added: &HashSet<usize>,
) -> Option<ResolutionGroup> {
    if consumed_removed.contains(&removed_index) {
        return None;
    }
    let available = available_candidate_indexes(
        &by_removed[removed_index],
        candidates,
        consumed_removed,
        consumed_added,
    );
    let top = top_candidate_indexes(&available, candidates)?;
    if top.len() < 2 {
        return None;
    }
    for index in &top {
        let candidate = &candidates[*index];
        let reverse_available = available_candidate_indexes(
            &by_added[candidate.added_index],
            candidates,
            consumed_removed,
            consumed_added,
        );
        let reverse_top = top_candidate_indexes(&reverse_available, candidates)?;
        if reverse_top.len() != 1 || reverse_top[0] != *index {
            return None;
        }
    }

    Some(ResolutionGroup {
        kind: LineageEventKind::Split,
        removed_indices: vec![removed_index],
        added_indices: top
            .iter()
            .map(|index| candidates[*index].added_index)
            .collect(),
        confidence: average_score(top.iter().map(|index| candidates[*index].score)),
        evidence: merge_evidence(
            top.iter()
                .flat_map(|index| candidates[*index].evidence.clone()),
        ),
    })
}

fn merge_group(
    added_index: usize,
    candidates: &[CandidateMatch],
    by_removed: &[Vec<usize>],
    by_added: &[Vec<usize>],
    consumed_removed: &HashSet<usize>,
    consumed_added: &HashSet<usize>,
) -> Option<ResolutionGroup> {
    if consumed_added.contains(&added_index) {
        return None;
    }
    let available = available_candidate_indexes(
        &by_added[added_index],
        candidates,
        consumed_removed,
        consumed_added,
    );
    let top = top_candidate_indexes(&available, candidates)?;
    if top.len() < 2 {
        return None;
    }
    for index in &top {
        let candidate = &candidates[*index];
        let reverse_available = available_candidate_indexes(
            &by_removed[candidate.removed_index],
            candidates,
            consumed_removed,
            consumed_added,
        );
        let reverse_top = top_candidate_indexes(&reverse_available, candidates)?;
        if reverse_top.len() != 1 || reverse_top[0] != *index {
            return None;
        }
    }

    Some(ResolutionGroup {
        kind: LineageEventKind::Merged,
        removed_indices: top
            .iter()
            .map(|index| candidates[*index].removed_index)
            .collect(),
        added_indices: vec![added_index],
        confidence: average_score(top.iter().map(|index| candidates[*index].score)),
        evidence: merge_evidence(
            top.iter()
                .flat_map(|index| candidates[*index].evidence.clone()),
        ),
    })
}

fn unique_group(
    candidate_index: usize,
    candidates: &[CandidateMatch],
    by_removed: &[Vec<usize>],
    by_added: &[Vec<usize>],
    consumed_removed: &HashSet<usize>,
    consumed_added: &HashSet<usize>,
) -> Option<ResolutionGroup> {
    let candidate = &candidates[candidate_index];
    if consumed_removed.contains(&candidate.removed_index)
        || consumed_added.contains(&candidate.added_index)
        || candidate.score < STRONG_MATCH_THRESHOLD
    {
        return None;
    }

    let removed_available = available_candidate_indexes(
        &by_removed[candidate.removed_index],
        candidates,
        consumed_removed,
        consumed_added,
    );
    let removed_top = top_candidate_indexes(&removed_available, candidates)?;
    if removed_top.len() != 1 || removed_top[0] != candidate_index {
        return None;
    }

    let added_available = available_candidate_indexes(
        &by_added[candidate.added_index],
        candidates,
        consumed_removed,
        consumed_added,
    );
    let added_top = top_candidate_indexes(&added_available, candidates)?;
    if added_top.len() != 1 || added_top[0] != candidate_index {
        return None;
    }

    Some(ResolutionGroup {
        kind: candidate.kind.clone(),
        removed_indices: vec![candidate.removed_index],
        added_indices: vec![candidate.added_index],
        confidence: candidate.score,
        evidence: candidate.evidence.clone(),
    })
}

fn ambiguous_group_for_added(
    added_index: usize,
    candidates: &[CandidateMatch],
    by_added: &[Vec<usize>],
    consumed_removed: &HashSet<usize>,
    consumed_added: &HashSet<usize>,
) -> Option<ResolutionGroup> {
    if consumed_added.contains(&added_index) {
        return None;
    }
    let available = available_candidate_indexes(
        &by_added[added_index],
        candidates,
        consumed_removed,
        consumed_added,
    );
    let top = top_candidate_indexes(&available, candidates)?;
    if top.len() < 2 {
        return None;
    }

    Some(ResolutionGroup {
        kind: LineageEventKind::Ambiguous,
        removed_indices: dedupe_usize(
            top.iter()
                .map(|index| candidates[*index].removed_index)
                .collect(),
        ),
        added_indices: vec![added_index],
        confidence: average_score(top.iter().map(|index| candidates[*index].score)),
        evidence: merge_evidence(
            top.iter()
                .flat_map(|index| candidates[*index].evidence.clone()),
        ),
    })
}

fn ambiguous_group_for_removed(
    removed_index: usize,
    candidates: &[CandidateMatch],
    by_removed: &[Vec<usize>],
    consumed_removed: &HashSet<usize>,
    consumed_added: &HashSet<usize>,
) -> Option<ResolutionGroup> {
    if consumed_removed.contains(&removed_index) {
        return None;
    }
    let available = available_candidate_indexes(
        &by_removed[removed_index],
        candidates,
        consumed_removed,
        consumed_added,
    );
    let top = top_candidate_indexes(&available, candidates)?;
    if top.len() < 2 {
        return None;
    }

    Some(ResolutionGroup {
        kind: LineageEventKind::Ambiguous,
        removed_indices: vec![removed_index],
        added_indices: dedupe_usize(
            top.iter()
                .map(|index| candidates[*index].added_index)
                .collect(),
        ),
        confidence: average_score(top.iter().map(|index| candidates[*index].score)),
        evidence: merge_evidence(
            top.iter()
                .flat_map(|index| candidates[*index].evidence.clone()),
        ),
    })
}

fn mark_group_consumed(
    group: &ResolutionGroup,
    consumed_removed: &mut HashSet<usize>,
    consumed_added: &mut HashSet<usize>,
) {
    consumed_removed.extend(group.removed_indices.iter().copied());
    consumed_added.extend(group.added_indices.iter().copied());
}

fn classify_change(
    change_set: &ObservedChangeSet,
    before: &ObservedNode,
    after: &ObservedNode,
) -> LineageEventKind {
    if has_path_transition(change_set) || before.node.file != after.node.file {
        LineageEventKind::Moved
    } else if last_path_segment(&before.node.id.path) != last_path_segment(&after.node.id.path) {
        LineageEventKind::Renamed
    } else if before.node.id.path != after.node.id.path {
        LineageEventKind::Reparented
    } else {
        LineageEventKind::Updated
    }
}

fn fingerprint_match(
    before: &SymbolFingerprint,
    after: &SymbolFingerprint,
) -> Option<(f32, Vec<LineageEvidence>)> {
    if before.signature_hash != after.signature_hash {
        return None;
    }

    let mut score = 0.4;
    let mut evidence = vec![LineageEvidence::SignatureMatch];

    if before.body_hash.is_some() && before.body_hash == after.body_hash {
        score += 0.3;
        evidence.push(LineageEvidence::BodyHashMatch);
    }
    if before.skeleton_hash.is_some() && before.skeleton_hash == after.skeleton_hash {
        score += 0.2;
        evidence.push(LineageEvidence::SkeletonMatch);
    }
    if before.child_shape_hash.is_some() && before.child_shape_hash == after.child_shape_hash {
        score += 0.1;
    }
    if before == after {
        evidence.insert(0, LineageEvidence::FingerprintMatch);
    }

    Some((score, evidence))
}

fn dedupe_evidence(mut evidence: Vec<LineageEvidence>) -> Vec<LineageEvidence> {
    let mut deduped = Vec::new();
    for item in evidence.drain(..) {
        if !deduped.contains(&item) {
            deduped.push(item);
        }
    }
    deduped
}

fn merge_evidence<I>(evidence: I) -> Vec<LineageEvidence>
where
    I: IntoIterator<Item = LineageEvidence>,
{
    let mut merged = evidence.into_iter().collect::<Vec<_>>();
    merged.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
    merged.dedup();
    merged
}

fn dedupe_usize(mut values: Vec<usize>) -> Vec<usize> {
    values.sort_unstable();
    values.dedup();
    values
}

fn dedupe_node_ids(mut values: Vec<NodeId>) -> Vec<NodeId> {
    values.sort_by(|left, right| {
        left.crate_name
            .cmp(&right.crate_name)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.kind.to_string().cmp(&right.kind.to_string()))
    });
    values.dedup();
    values
}

fn average_score<I>(scores: I) -> f32
where
    I: IntoIterator<Item = f32>,
{
    let mut total = 0.0;
    let mut count = 0.0;
    for score in scores {
        total += score;
        count += 1.0;
    }
    if count == 0.0 {
        0.0
    } else {
        total / count
    }
}

fn average_revive_score(candidates: &[&ReviveCandidate]) -> f32 {
    average_score(candidates.iter().map(|candidate| candidate.score))
}

fn choose_lineage(mut lineages: Vec<LineageId>) -> LineageId {
    lineages.sort_by(|left, right| left.0.cmp(&right.0));
    lineages
        .into_iter()
        .next()
        .expect("lineage selection requires at least one lineage")
}

pub(crate) fn last_path_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once("::").map(|(parent, _)| parent)
}

fn path_lineage_index(
    node_to_lineage: &HashMap<NodeId, LineageId>,
) -> HashMap<(String, String), LineageId> {
    let mut index = HashMap::new();
    for (node, lineage) in node_to_lineage {
        index
            .entry((node.crate_name.to_string(), node.path.to_string()))
            .or_insert_with(|| lineage.clone());
    }
    index
}

fn container_lineage_for_node(
    node: &NodeId,
    path_lineages: &HashMap<(String, String), LineageId>,
) -> Option<LineageId> {
    let parent = parent_path(node.path.as_str())?;
    path_lineages
        .get(&(node.crate_name.to_string(), parent.to_string()))
        .cloned()
}

fn shares_container_lineage(
    before: &ObservedNode,
    after: &ObservedNode,
    path_lineages: &HashMap<(String, String), LineageId>,
) -> bool {
    container_lineage_for_node(&before.node.id, path_lineages)
        .zip(container_lineage_for_node(&after.node.id, path_lineages))
        .is_some_and(|(left, right)| left == right)
}

fn has_path_transition(change_set: &ObservedChangeSet) -> bool {
    change_set
        .previous_path
        .as_ref()
        .zip(change_set.current_path.as_ref())
        .is_some_and(|(previous, current)| previous != current)
}

fn file_hint_evidence(change_set: &ObservedChangeSet) -> Option<LineageEvidence> {
    if !has_path_transition(change_set) {
        return None;
    }
    Some(match change_set.trigger {
        ChangeTrigger::GitCheckout | ChangeTrigger::GitCommitImport => {
            LineageEvidence::GitRenameHint
        }
        _ => LineageEvidence::FileMoveHint,
    })
}
