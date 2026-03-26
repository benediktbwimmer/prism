use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use prism_ir::{Node, NodeId};
use prism_parser::ParseResult;
use prism_store::{FileState, Graph};

use crate::PendingFileParse;

pub(crate) fn detect_moved_files(
    graph: &Graph,
    seen_files: &HashSet<PathBuf>,
    pending: &mut [PendingFileParse],
) -> HashSet<PathBuf> {
    let mut old_by_hash = HashMap::<u64, Vec<PathBuf>>::new();
    for tracked in graph.tracked_files() {
        if seen_files.contains(&tracked) {
            continue;
        }
        if let Some(record) = graph.file_record(&tracked) {
            old_by_hash.entry(record.hash).or_default().push(tracked);
        }
    }

    let mut moved_paths = HashSet::new();
    for pending_file in pending
        .iter_mut()
        .filter(|pending_file| graph.file_record(&pending_file.path).is_none())
    {
        let Some(candidates) = old_by_hash.get(&pending_file.hash) else {
            continue;
        };
        let available = candidates
            .iter()
            .filter(|candidate| !moved_paths.contains(*candidate))
            .collect::<Vec<_>>();
        if available.len() == 1 {
            let previous = (*available[0]).clone();
            pending_file.previous_path = Some(previous.clone());
            moved_paths.insert(previous);
        }
    }

    moved_paths
}

pub(crate) fn infer_reanchors(previous: &FileState, parsed: &ParseResult) -> Vec<(NodeId, NodeId)> {
    let previous_nodes = previous
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let parsed_nodes = parsed
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();

    let mut matched_old = HashSet::<NodeId>::new();
    let mut matched_new = HashSet::<NodeId>::new();
    let mut reanchors = Vec::<(NodeId, NodeId)>::new();
    let mut old_by_fingerprint = HashMap::<prism_parser::NodeFingerprint, Vec<NodeId>>::new();
    let mut new_by_fingerprint = HashMap::<prism_parser::NodeFingerprint, Vec<NodeId>>::new();

    for node in previous
        .nodes
        .iter()
        .filter(|node| parsed_nodes.contains_key(&node.id))
    {
        matched_old.insert(node.id.clone());
        matched_new.insert(node.id.clone());
    }

    for (id, fingerprint) in &previous.record.fingerprints {
        if previous_nodes.contains_key(id) {
            old_by_fingerprint
                .entry(fingerprint.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for (id, fingerprint) in &parsed.fingerprints {
        if parsed_nodes.contains_key(id) {
            new_by_fingerprint
                .entry(fingerprint.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for (fingerprint, old_ids) in &old_by_fingerprint {
        let Some(new_ids) = new_by_fingerprint.get(fingerprint) else {
            continue;
        };
        let available_old = old_ids
            .iter()
            .filter(|id| !matched_old.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let available_new = new_ids
            .iter()
            .filter(|id| !matched_new.contains(*id))
            .cloned()
            .collect::<Vec<_>>();

        if available_old.len() == 1 && available_new.len() == 1 {
            let old = available_old[0].clone();
            let new = available_new[0].clone();
            matched_old.insert(old.clone());
            matched_new.insert(new.clone());
            if old != new {
                reanchors.push((old, new));
            }
        }
    }

    for (fingerprint, old_ids) in old_by_fingerprint {
        let Some(new_ids) = new_by_fingerprint.get(&fingerprint) else {
            continue;
        };

        for old_id in old_ids {
            if matched_old.contains(&old_id) {
                continue;
            }

            let Some(old_node) = previous_nodes.get(&old_id) else {
                continue;
            };
            let best = new_ids
                .iter()
                .filter(|new_id| !matched_new.contains(*new_id))
                .filter_map(|new_id| {
                    let new_node = parsed_nodes.get(new_id)?;
                    Some((score_reanchor_candidate(old_node, new_node), new_id.clone()))
                })
                .filter(|(score, _)| *score >= 40)
                .max_by_key(|(score, _)| *score);

            if let Some((_, new_id)) = best {
                matched_old.insert(old_id.clone());
                matched_new.insert(new_id.clone());
                if old_id != new_id {
                    reanchors.push((old_id, new_id));
                }
            }
        }
    }

    reanchors
}

fn score_reanchor_candidate(old: &Node, new: &Node) -> i32 {
    if old.kind != new.kind || old.language != new.language {
        return 0;
    }

    let mut score = 0;
    if old.name == new.name {
        score += 20;
    }
    if old.id.crate_name == new.id.crate_name {
        score += 10;
    }
    if parent_path(old.id.path.as_str()) == parent_path(new.id.path.as_str()) {
        score += 10;
    }

    let start_delta = old.span.start.abs_diff(new.span.start);
    score += (20 - start_delta.min(20)) as i32;

    let end_delta = old.span.end.abs_diff(new.span.end);
    score += (20 - end_delta.min(20)) as i32;

    score
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once("::")
        .map(|(parent, _)| parent)
        .unwrap_or(path)
}
