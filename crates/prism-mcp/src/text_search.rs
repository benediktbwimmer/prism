use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use globset::{GlobBuilder, GlobMatcher};
use ignore::WalkBuilder;
use prism_js::{SourceExcerptView, SourceLocationView, TextSearchMatchView};
use prism_query::{source_excerpt_for_span, source_location_for_span};
use regex::{Regex, RegexBuilder};

use crate::{QueryHost, SearchTextArgs, DEFAULT_SEARCH_LIMIT};

const DEFAULT_TEXT_SEARCH_CONTEXT_LINES: usize = 1;
const DEFAULT_TEXT_SEARCH_MAX_CHARS: usize = 240;

pub(crate) struct TextSearchOutcome {
    pub(crate) results: Vec<TextSearchMatchView>,
    pub(crate) requested: usize,
    pub(crate) applied: usize,
    pub(crate) limit_hit: bool,
}

pub(crate) struct PathSearchOutcome {
    pub(crate) results: Vec<String>,
}

pub(crate) fn search_text(
    host: &QueryHost,
    args: SearchTextArgs,
    max_limit: usize,
) -> Result<TextSearchOutcome> {
    let workspace = host
        .workspace_session()
        .ok_or_else(|| anyhow!("text search requires a workspace-backed PRISM session"))?;
    if args.query.trim().is_empty() {
        return Err(anyhow!("query must be a non-empty string"));
    }

    let case_sensitive = args.case_sensitive.unwrap_or(false);
    let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    let applied = requested.min(max_limit);
    let context_lines = args
        .context_lines
        .unwrap_or(DEFAULT_TEXT_SEARCH_CONTEXT_LINES);
    let matcher = compile_search_pattern(&args.query, args.regex.unwrap_or(false), case_sensitive)?;
    let glob = compile_glob_matcher(args.glob.as_deref(), case_sensitive)?;
    let path_filter = args
        .path
        .filter(|value| !value.trim().is_empty())
        .map(|value| normalize_path_filter(&value, case_sensitive));

    let root = workspace.root();
    let mut files = workspace_files(root, path_filter.as_deref(), glob.as_ref(), case_sensitive)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut results = Vec::new();
    let mut limit_hit = false;
    for (relative_path, absolute_path) in files {
        let Some(source) = read_searchable_file(&absolute_path)? else {
            continue;
        };
        for found in matcher.find_iter(&source) {
            results.push(TextSearchMatchView {
                path: relative_path.clone(),
                location: source_location_view(source_location_for_span(
                    &source,
                    found.start(),
                    found.end(),
                )),
                excerpt: source_excerpt_view(source_excerpt_for_span(
                    &source,
                    found.start(),
                    found.end(),
                    context_lines,
                    DEFAULT_TEXT_SEARCH_MAX_CHARS,
                )),
            });
            if results.len() >= applied {
                limit_hit = true;
                return Ok(TextSearchOutcome {
                    results,
                    requested,
                    applied,
                    limit_hit,
                });
            }
        }
    }

    Ok(TextSearchOutcome {
        results,
        requested,
        applied,
        limit_hit,
    })
}

pub(crate) fn search_workspace_paths(
    host: &QueryHost,
    args: SearchTextArgs,
    max_limit: usize,
) -> Result<PathSearchOutcome> {
    let workspace = host
        .workspace_session()
        .ok_or_else(|| anyhow!("path search requires a workspace-backed PRISM session"))?;
    if args.query.trim().is_empty() {
        return Err(anyhow!("query must be a non-empty string"));
    }

    let case_sensitive = args.case_sensitive.unwrap_or(false);
    let requested = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    let applied = requested.min(max_limit);
    let glob = compile_glob_matcher(args.glob.as_deref(), case_sensitive)?;
    let path_filter = args
        .path
        .filter(|value| !value.trim().is_empty())
        .map(|value| normalize_path_filter(&value, case_sensitive));
    let normalized_query = normalize_path_query(&args.query, case_sensitive);
    let query_tokens = normalized_query
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    let root = workspace.root();
    let mut files = workspace_files(root, path_filter.as_deref(), glob.as_ref(), case_sensitive)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));

    let mut results = Vec::new();
    for (relative_path, _) in files {
        if !workspace_path_matches_query(
            &relative_path,
            &normalized_query,
            &query_tokens,
            case_sensitive,
        ) {
            continue;
        }
        results.push(relative_path);
        if results.len() >= applied {
            break;
        }
    }

    Ok(PathSearchOutcome { results })
}

fn workspace_files(
    root: &Path,
    path_filter: Option<&str>,
    glob: Option<&GlobMatcher>,
    case_sensitive: bool,
) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for entry in WalkBuilder::new(root).build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let absolute = entry.path();
        let relative = absolute
            .strip_prefix(root)
            .with_context(|| {
                format!(
                    "failed to compute workspace-relative path for {}",
                    absolute.display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(filter) = path_filter {
            let candidate = normalize_path_filter(&relative, case_sensitive);
            if !candidate.contains(filter) {
                continue;
            }
        }
        if let Some(glob) = glob {
            let relative_path = Path::new(&relative);
            if !glob.is_match(relative_path) {
                continue;
            }
        }
        files.push((relative, absolute.to_path_buf()));
    }
    Ok(files)
}

fn read_searchable_file(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read searchable file {}", path.display()))
        }
    }
}

fn compile_search_pattern(query: &str, is_regex: bool, case_sensitive: bool) -> Result<Regex> {
    let pattern = if is_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let mut builder = RegexBuilder::new(&pattern);
    builder.case_insensitive(!case_sensitive);
    builder.multi_line(true);
    builder.dot_matches_new_line(true);
    builder
        .build()
        .with_context(|| format!("invalid search pattern `{query}`"))
}

fn compile_glob_matcher(glob: Option<&str>, case_sensitive: bool) -> Result<Option<GlobMatcher>> {
    let Some(glob) = glob.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let matcher = GlobBuilder::new(glob)
        .case_insensitive(!case_sensitive)
        .build()
        .with_context(|| format!("invalid glob pattern `{glob}`"))?
        .compile_matcher();
    Ok(Some(matcher))
}

fn normalize_path_filter(value: &str, case_sensitive: bool) -> String {
    let normalized = value.replace('\\', "/");
    if case_sensitive {
        normalized
    } else {
        normalized.to_ascii_lowercase()
    }
}

fn normalize_path_query(value: &str, case_sensitive: bool) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_lower = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && previous_was_lower {
                normalized.push(' ');
            }
            normalized.push(if case_sensitive {
                ch
            } else {
                ch.to_ascii_lowercase()
            });
            previous_was_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            normalized.push(' ');
            previous_was_lower = false;
        }
    }
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn workspace_path_matches_query(
    relative_path: &str,
    normalized_query: &str,
    query_tokens: &[&str],
    case_sensitive: bool,
) -> bool {
    if normalized_query.is_empty() {
        return false;
    }
    let normalized_path = normalize_path_query(relative_path, case_sensitive);
    if normalized_path.contains(normalized_query) {
        return true;
    }
    let basename = Path::new(relative_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(relative_path);
    let basename_without_extension = basename
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(basename);
    let normalized_basename = normalize_path_query(basename_without_extension, case_sensitive);
    normalized_basename.contains(normalized_query)
        || (!query_tokens.is_empty()
            && query_tokens
                .iter()
                .all(|token| normalized_path.contains(token)))
}

fn source_location_view(location: prism_query::SourceLocation) -> SourceLocationView {
    SourceLocationView {
        start_line: location.start_line,
        start_column: location.start_column,
        end_line: location.end_line,
        end_column: location.end_column,
    }
}

fn source_excerpt_view(excerpt: prism_query::SourceExcerpt) -> SourceExcerptView {
    SourceExcerptView {
        text: excerpt.text,
        start_line: excerpt.start_line,
        end_line: excerpt.end_line,
        truncated: excerpt.truncated,
    }
}
