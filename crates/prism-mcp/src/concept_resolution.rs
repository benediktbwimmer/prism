use prism_query::{ConceptResolution, Prism};

use crate::{session_state::SessionTaskState, SessionState};

const MIN_RESOLUTION_LIMIT: usize = 5;

pub(crate) fn resolve_concepts_for_session(
    prism: &Prism,
    session: &SessionState,
    query: &str,
    limit: usize,
) -> Vec<ConceptResolution> {
    if limit == 0 {
        return Vec::new();
    }

    let fetch_limit = limit.saturating_mul(3).max(MIN_RESOLUTION_LIMIT);
    let mut resolutions = prism.resolve_concepts(query, fetch_limit);
    rerank_for_session(
        resolutions.as_mut_slice(),
        session.current_task_state().as_ref(),
    );
    resolutions.truncate(limit);
    resolutions
}

pub(crate) fn concept_resolution_is_ambiguous(resolutions: &[ConceptResolution]) -> bool {
    let [top, second, ..] = resolutions else {
        return false;
    };
    second.score.saturating_add(35) >= top.score
        || (top.score > 0 && second.score.saturating_mul(100) >= top.score.saturating_mul(85))
}

pub(crate) fn weak_concept_match_reason(score: i32) -> Option<&'static str> {
    if score < 120 {
        Some("only weak concept signals matched")
    } else if score < 180 {
        Some("concept resolution is plausible but low-confidence")
    } else {
        None
    }
}

fn rerank_for_session(resolutions: &mut [ConceptResolution], task: Option<&SessionTaskState>) {
    let Some(task) = task else {
        return;
    };
    let mut context = String::new();
    if let Some(description) = task.description.as_deref() {
        context.push_str(description);
        context.push(' ');
    }
    if !task.tags.is_empty() {
        context.push_str(&task.tags.join(" "));
    }
    let context_tokens = normalized_tokens(&context);
    if context_tokens.is_empty() {
        return;
    }

    for resolution in resolutions.iter_mut() {
        let (boost, matched_provenance) = concept_context_boost(resolution, &context_tokens, task);
        if boost > 0 {
            resolution.score += boost;
            push_reason(&mut resolution.reasons, "linked to current task context");
            if matched_provenance {
                push_reason(&mut resolution.reasons, "matched current task provenance");
            }
        }
    }

    resolutions.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.packet.confidence.total_cmp(&left.packet.confidence))
            .then_with(|| left.packet.handle.cmp(&right.packet.handle))
    });
}

fn concept_context_boost(
    resolution: &ConceptResolution,
    context_tokens: &[String],
    task: &SessionTaskState,
) -> (i32, bool) {
    let context = context_tokens
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let mut boost = 0;
    boost += overlap_points(
        &context,
        &normalized_tokens(&resolution.packet.canonical_name),
        18,
        54,
    );
    for alias in &resolution.packet.aliases {
        boost += overlap_points(&context, &normalized_tokens(alias), 14, 42);
    }
    boost += overlap_points(
        &context,
        &normalized_tokens(&resolution.packet.summary),
        8,
        24,
    );
    for member in resolution
        .packet
        .core_members
        .iter()
        .chain(resolution.packet.supporting_members.iter())
        .chain(resolution.packet.likely_tests.iter())
    {
        let label = member
            .path
            .rsplit("::")
            .next()
            .unwrap_or(member.path.as_str());
        boost += overlap_points(&context, &normalized_tokens(label), 10, 30);
    }
    let matched_provenance =
        resolution.packet.provenance.task_id.as_deref() == Some(task.id.0.as_str());
    if matched_provenance {
        boost += 30;
    }
    (boost.min(120), matched_provenance)
}

fn overlap_points(
    query_tokens: &std::collections::HashSet<String>,
    candidate_tokens: &[String],
    per_token: i32,
    max_score: i32,
) -> i32 {
    let overlap = candidate_tokens
        .iter()
        .filter(|token| query_tokens.contains(*token))
        .count() as i32;
    (overlap * per_token).min(max_score)
}

fn normalized_tokens(value: &str) -> Vec<String> {
    let mut normalized = String::new();
    let mut previous: Option<char> = None;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if let Some(previous) = previous {
                let boundary = (previous.is_ascii_lowercase() && ch.is_ascii_uppercase())
                    || (previous.is_ascii_digit() && ch.is_ascii_alphabetic())
                    || (previous.is_ascii_alphabetic() && ch.is_ascii_digit());
                if boundary {
                    normalized.push(' ');
                }
            }
            normalized.push(ch.to_ascii_lowercase());
        } else {
            normalized.push(' ');
        }
        previous = Some(ch);
    }

    normalized
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

fn push_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}
