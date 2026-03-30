use std::path::Path;

use prism_core::WorkspaceBoundaryRegion;

use crate::QueryExecution;

pub(crate) fn append_boundary_notes_for_paths(
    execution: &QueryExecution,
    paths: &[String],
    notes: &mut Vec<String>,
) {
    let Some(summary) = execution.workspace_materialization_summary() else {
        return;
    };
    for path in paths {
        let Some(boundary) = boundary_for_requested_path(&summary.boundaries, path) else {
            continue;
        };
        notes.push(boundary_note(path, boundary));
    }
}

fn boundary_for_requested_path<'a>(
    boundaries: &'a [WorkspaceBoundaryRegion],
    requested_path: &str,
) -> Option<&'a WorkspaceBoundaryRegion> {
    let requested = Path::new(requested_path);
    boundaries
        .iter()
        .filter(|boundary| requested.starts_with(&boundary.path))
        .max_by_key(|boundary| boundary.path.components().count())
}

fn boundary_note(requested_path: &str, boundary: &WorkspaceBoundaryRegion) -> String {
    let boundary_path = boundary.path.display();
    match boundary.scope_state.as_str() {
        "out_of_scope" => format!(
            "Requested path `{requested_path}` is outside the current indexed scope under boundary `{boundary_path}`; semantic follow-up is partial until that region is materialized."
        ),
        _ => match boundary.materialization_state.as_str() {
            "known_unmaterialized" => format!(
                "Requested path `{requested_path}` falls under known but unmaterialized boundary `{boundary_path}`; semantic follow-up is partial until that region is indexed."
            ),
            "sparse" => format!(
                "Requested path `{requested_path}` falls under sparsely materialized boundary `{boundary_path}` ({}/{} files indexed); semantic follow-up may be incomplete.",
                boundary.materialized_file_count, boundary.known_file_count
            ),
            _ => format!(
                "Requested path `{requested_path}` falls under boundary `{boundary_path}` with materialization state `{}`; semantic follow-up may be incomplete.",
                boundary.materialization_state
            ),
        },
    }
}
