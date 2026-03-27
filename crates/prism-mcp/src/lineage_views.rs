use anyhow::Result;
use prism_ir::{LineageEvent, LineageEvidence, LineageEventKind, NodeId};
use prism_js::{LineageEventView, LineageEvidenceView, LineageStatus, LineageView, SymbolView};
use prism_query::Prism;

use crate::{node_id_view, symbol_for, symbol_view};

pub(crate) fn lineage_view(prism: &Prism, id: &NodeId) -> Result<Option<LineageView>> {
    let Some(lineage) = prism.lineage_of(id) else {
        return Ok(None);
    };
    let current = symbol_for(prism, id)?;
    let current = symbol_view(prism, &current)?;
    let events = prism.lineage_history(&lineage);
    let status = lineage_status(&events);
    let history = events.iter().map(lineage_event_view).collect::<Vec<_>>();
    Ok(Some(LineageView {
        lineage_id: lineage.0.to_string(),
        current: current.clone(),
        status,
        summary: lineage_summary(&current, status, &history),
        uncertainty: lineage_uncertainty(status, &history),
        history,
    }))
}

pub(crate) fn lineage_event_view(event: &LineageEvent) -> LineageEventView {
    let evidence_details = event
        .evidence
        .iter()
        .cloned()
        .map(lineage_evidence_view)
        .collect::<Vec<_>>();
    LineageEventView {
        event_id: event.meta.id.0.to_string(),
        ts: event.meta.ts,
        kind: format!("{:?}", event.kind),
        confidence: event.confidence,
        before: event.before.iter().cloned().map(node_id_view).collect(),
        after: event.after.iter().cloned().map(node_id_view).collect(),
        evidence: evidence_details
            .iter()
            .map(|detail| detail.code.clone())
            .collect(),
        evidence_details,
        summary: lineage_event_summary(event),
    }
}

pub(crate) fn lineage_status(events: &[LineageEvent]) -> LineageStatus {
    if events
        .iter()
        .any(|event| matches!(event.kind, LineageEventKind::Ambiguous))
    {
        LineageStatus::Ambiguous
    } else if events
        .last()
        .is_some_and(|event| matches!(event.kind, LineageEventKind::Died))
    {
        LineageStatus::Dead
    } else {
        LineageStatus::Active
    }
}

fn lineage_summary(
    current: &SymbolView,
    status: LineageStatus,
    history: &[LineageEventView],
) -> String {
    let status_text = match status {
        LineageStatus::Active => "Active lineage",
        LineageStatus::Dead => "Dead lineage",
        LineageStatus::Ambiguous => "Ambiguous lineage",
    };
    match history.last() {
        Some(event) => format!(
            "{status_text} for {}. Latest event: {}",
            current.id.path, event.summary
        ),
        None => format!(
            "{status_text} for {} with no recorded transitions yet.",
            current.id.path
        ),
    }
}

fn lineage_uncertainty(status: LineageStatus, history: &[LineageEventView]) -> Vec<String> {
    let mut reasons = Vec::new();
    if matches!(status, LineageStatus::Ambiguous) {
        reasons.push(
            "At least one lineage decision is ambiguous, so earlier names or locations may not be definitive."
                .to_string(),
        );
    }
    if history
        .iter()
        .any(|event| event.kind.eq_ignore_ascii_case("split"))
    {
        reasons.push(
            "This lineage includes a split event, so one previous symbol continued into multiple current symbols."
                .to_string(),
        );
    }
    if history
        .iter()
        .any(|event| event.kind.eq_ignore_ascii_case("merged"))
    {
        reasons.push(
            "This lineage includes a merge event, so multiple previous symbols collapsed into one current lineage."
                .to_string(),
        );
    }
    reasons
}

fn lineage_event_summary(event: &LineageEvent) -> String {
    let before = summarize_nodes(&event.before);
    let after = summarize_nodes(&event.after);
    match event.kind {
        LineageEventKind::Born => format!("Started a new lineage at {after}."),
        LineageEventKind::Updated => format!("Continued in place at {after}."),
        LineageEventKind::Renamed => format!("Renamed from {before} to {after}."),
        LineageEventKind::Moved => format!("Moved from {before} to {after}."),
        LineageEventKind::Reparented => format!("Reparented from {before} to {after}."),
        LineageEventKind::Split => format!("Split from {before} into {after}."),
        LineageEventKind::Merged => format!("Merged {before} into {after}."),
        LineageEventKind::Died => format!("Ended after {before} with no current match."),
        LineageEventKind::Revived => format!("Revived at {after} after being absent."),
        LineageEventKind::Ambiguous => {
            format!("Ambiguous match between previous {before} and current {after}.")
        }
    }
}

fn lineage_evidence_view(evidence: LineageEvidence) -> LineageEvidenceView {
    match evidence {
        LineageEvidence::ExactNodeId => LineageEvidenceView {
            code: "exact_node_id".to_string(),
            label: "Exact Node Id".to_string(),
            detail: "The resolver saw the same node identity before and after the change.".to_string(),
        },
        LineageEvidence::FingerprintMatch => LineageEvidenceView {
            code: "fingerprint_match".to_string(),
            label: "Exact Fingerprint Match".to_string(),
            detail: "Signature and structural hashes matched exactly across the change.".to_string(),
        },
        LineageEvidence::SignatureMatch => LineageEvidenceView {
            code: "signature_match".to_string(),
            label: "Signature Match".to_string(),
            detail: "The symbol signature stayed compatible across the change.".to_string(),
        },
        LineageEvidence::BodyHashMatch => LineageEvidenceView {
            code: "body_hash_match".to_string(),
            label: "Body Hash Match".to_string(),
            detail: "The resolver matched the body shape or implementation hash.".to_string(),
        },
        LineageEvidence::SkeletonMatch => LineageEvidenceView {
            code: "skeleton_match".to_string(),
            label: "Skeleton Match".to_string(),
            detail: "The resolver matched the symbol skeleton even if details moved around.".to_string(),
        },
        LineageEvidence::SameContainerLineage => LineageEvidenceView {
            code: "same_container_lineage".to_string(),
            label: "Same Container Continuity".to_string(),
            detail: "The surrounding container lineage stayed stable, which strengthened the match.".to_string(),
        },
        LineageEvidence::GitRenameHint => LineageEvidenceView {
            code: "git_rename_hint".to_string(),
            label: "Git Rename Hint".to_string(),
            detail: "The change looked like a Git-driven rename or move, which strengthened the lineage decision.".to_string(),
        },
        LineageEvidence::FileMoveHint => LineageEvidenceView {
            code: "file_move_hint".to_string(),
            label: "File Move Hint".to_string(),
            detail: "The file path changed in a way that suggested a move, which strengthened the lineage decision.".to_string(),
        },
    }
}

fn summarize_nodes(nodes: &[NodeId]) -> String {
    match nodes {
        [] => "nothing".to_string(),
        [only] => only.path.to_string(),
        many => format!("{} symbols", many.len()),
    }
}
