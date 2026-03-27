use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use prism_js::{SourceExcerptView, SourceLocationView, SourceSliceView};
use prism_query::{source_excerpt_for_line_range, source_slice_around_line};

use crate::{FileAroundArgs, FileReadArgs, QueryHost};

pub(crate) const DEFAULT_FILE_READ_MAX_CHARS: usize = 1400;
pub(crate) const DEFAULT_FILE_AROUND_MAX_CHARS: usize = 1200;
pub(crate) const DEFAULT_FILE_AROUND_CONTEXT_LINES: usize = 2;

pub(crate) fn file_read(host: &QueryHost, args: FileReadArgs) -> Result<SourceExcerptView> {
    let path = resolve_workspace_path(host, &args.path)?;
    let source = read_workspace_file(&path)?;
    let start_line = args.start_line.unwrap_or(1);
    let end_line = args.end_line.unwrap_or(usize::MAX);
    if start_line == 0 {
        return Err(anyhow!("startLine must be at least 1"));
    }
    if end_line == 0 {
        return Err(anyhow!("endLine must be at least 1"));
    }
    if end_line < start_line {
        return Err(anyhow!(
            "endLine must be greater than or equal to startLine"
        ));
    }

    let excerpt = source_excerpt_for_line_range(
        &source,
        start_line,
        end_line,
        args.max_chars.unwrap_or(DEFAULT_FILE_READ_MAX_CHARS),
    );
    Ok(SourceExcerptView {
        text: excerpt.text,
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        truncated: excerpt.truncated,
    })
}

pub(crate) fn file_around(host: &QueryHost, args: FileAroundArgs) -> Result<SourceSliceView> {
    let path = resolve_workspace_path(host, &args.path)?;
    let source = read_workspace_file(&path)?;
    if args.line == 0 {
        return Err(anyhow!("line must be at least 1"));
    }

    let slice = source_slice_around_line(
        &source,
        args.line,
        args.before.unwrap_or(DEFAULT_FILE_AROUND_CONTEXT_LINES),
        args.after.unwrap_or(DEFAULT_FILE_AROUND_CONTEXT_LINES),
        args.max_chars.unwrap_or(DEFAULT_FILE_AROUND_MAX_CHARS),
    );
    Ok(SourceSliceView {
        text: slice.text,
        start_line: slice.start_line,
        end_line: slice.end_line,
        focus: source_location_view(slice.focus),
        relative_focus: source_location_view(slice.relative_focus),
        truncated: slice.truncated,
    })
}

fn resolve_workspace_path(host: &QueryHost, path: &str) -> Result<PathBuf> {
    let workspace = host
        .workspace
        .as_ref()
        .ok_or_else(|| anyhow!("file queries require a workspace-backed PRISM session"))?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("path must be a non-empty string"));
    }

    let candidate = if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        workspace.root().join(trimmed)
    };
    let canonical_root = fs::canonicalize(workspace.root()).with_context(|| {
        format!(
            "failed to resolve workspace root {}",
            workspace.root().display()
        )
    })?;
    let canonical_path = fs::canonicalize(&candidate)
        .with_context(|| format!("failed to resolve file path {}", candidate.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(anyhow!(
            "file path `{}` resolves outside the workspace root",
            path
        ));
    }
    Ok(canonical_path)
}

fn read_workspace_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read file {}", path.display()))
}

fn source_location_view(location: prism_query::SourceLocation) -> SourceLocationView {
    SourceLocationView {
        start_line: location.start_line,
        start_column: location.start_column,
        end_line: location.end_line,
        end_column: location.end_column,
    }
}
