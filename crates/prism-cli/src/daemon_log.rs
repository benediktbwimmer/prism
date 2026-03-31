use std::collections::VecDeque;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const DEFAULT_DAEMON_LOG_MAX_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_DAEMON_LOG_SEGMENT_MAX_BYTES: u64 = 8 * 1024 * 1024;
const DAEMON_LOG_MAX_BYTES_ENV: &str = "PRISM_MCP_DAEMON_LOG_MAX_BYTES";
const TAIL_READ_CHUNK_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone)]
struct SegmentFile {
    path: PathBuf,
    bytes: u64,
}

pub(crate) fn total_log_bytes(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for segment in archived_segments(path)? {
        total = total.saturating_add(segment.bytes);
    }
    if let Ok(metadata) = fs::metadata(path) {
        total = total.saturating_add(metadata.len());
    }
    Ok(total)
}

pub(crate) fn append_log_line(path: &Path, line: &str) -> Result<()> {
    append_log_line_with_max_bytes(path, line, configured_max_bytes())
}

fn append_log_line_with_max_bytes(path: &Path, line: &str, max_bytes: u64) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    rotate_active_segment_if_needed(path, line.len() + 1, max_bytes)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open daemon log {}", path.display()))?;
    writeln!(file, "{line}")
        .with_context(|| format!("failed to append daemon log {}", path.display()))?;
    prune_archived_segments(path, max_bytes)?;
    Ok(())
}

pub(crate) fn tail_lines(path: &Path, limit: usize) -> Result<Vec<String>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut lines = VecDeque::with_capacity(limit);
    for segment_path in segment_paths_in_reverse_order(path)? {
        let needed = limit.saturating_sub(lines.len());
        if needed == 0 {
            break;
        }
        let segment_lines = tail_lines_in_file(&segment_path, needed)?;
        for line in segment_lines.into_iter().rev() {
            if lines.len() == limit {
                break;
            }
            lines.push_front(line);
        }
    }
    Ok(lines.into_iter().collect())
}

fn configured_max_bytes() -> u64 {
    env::var(DAEMON_LOG_MAX_BYTES_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_DAEMON_LOG_MAX_BYTES)
}

fn rotate_active_segment_if_needed(
    path: &Path,
    next_write_bytes: usize,
    max_bytes: u64,
) -> Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let active_len = metadata.len();
    if active_len == 0 {
        return Ok(());
    }
    if active_len.saturating_add(next_write_bytes as u64) <= segment_max_bytes(max_bytes) {
        return Ok(());
    }
    let archive_path = archived_segment_path(path);
    fs::rename(path, &archive_path).with_context(|| {
        format!(
            "failed to rotate daemon log {} to {}",
            path.display(),
            archive_path.display()
        )
    })?;
    Ok(())
}

fn prune_archived_segments(path: &Path, max_bytes: u64) -> Result<()> {
    let mut total_bytes = total_log_bytes(path)?;
    if total_bytes <= max_bytes {
        return Ok(());
    }
    let prune_target = prune_target_bytes(max_bytes);
    for segment in archived_segments(path)? {
        if total_bytes <= prune_target {
            break;
        }
        fs::remove_file(&segment.path)
            .with_context(|| format!("failed to remove {}", segment.path.display()))?;
        total_bytes = total_bytes.saturating_sub(segment.bytes);
    }
    Ok(())
}

fn segment_paths_in_reverse_order(path: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if path.exists() {
        paths.push(path.to_path_buf());
    }
    paths.extend(
        archived_segments(path)?
            .into_iter()
            .rev()
            .map(|segment| segment.path),
    );
    Ok(paths)
}

fn archived_segments(path: &Path) -> Result<Vec<SegmentFile>> {
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    if !parent.exists() {
        return Ok(Vec::new());
    }
    let (prefix, suffix) = archived_segment_name_parts(path);
    let mut segments = Vec::new();
    for entry in
        fs::read_dir(parent).with_context(|| format!("failed to read {}", parent.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", parent.display()))?;
        let entry_path = entry.path();
        if entry_path == path {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !name.starts_with(&prefix) || !name.ends_with(&suffix) {
            continue;
        }
        let bytes = entry
            .metadata()
            .with_context(|| format!("failed to stat {}", entry_path.display()))?
            .len();
        segments.push(SegmentFile {
            path: entry_path,
            bytes,
        });
    }
    segments.sort_by(|left, right| left.path.file_name().cmp(&right.path.file_name()));
    Ok(segments)
}

fn archived_segment_path(path: &Path) -> PathBuf {
    let (prefix, suffix) = archived_segment_name_parts(path);
    path.with_file_name(format!("{prefix}{}{suffix}", chrono_like_token()))
}

fn archived_segment_name_parts(path: &Path) -> (String, String) {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("prism-mcp-daemon.log");
    match (
        path.file_stem().and_then(|value| value.to_str()),
        path.extension().and_then(|value| value.to_str()),
    ) {
        (Some(stem), Some(ext)) => (format!("{stem}."), format!(".{ext}")),
        _ => (format!("{file_name}."), String::new()),
    }
}

fn tail_lines_in_file(path: &Path, limit: usize) -> Result<Vec<String>> {
    if limit == 0 || !path.exists() {
        return Ok(Vec::new());
    }
    let mut file =
        File::open(path).with_context(|| format!("failed to open log file {}", path.display()))?;
    let mut position = file.metadata()?.len();
    let mut buffer = Vec::new();
    let mut chunk = vec![0u8; TAIL_READ_CHUNK_BYTES];
    while position > 0 {
        let read_len = TAIL_READ_CHUNK_BYTES.min(position as usize);
        position -= read_len as u64;
        file.seek(SeekFrom::Start(position))?;
        file.read_exact(&mut chunk[..read_len])?;
        let mut combined = Vec::with_capacity(read_len + buffer.len());
        combined.extend_from_slice(&chunk[..read_len]);
        combined.extend_from_slice(&buffer);
        buffer = combined;
        if buffer.iter().filter(|byte| **byte == b'\n').count() > limit {
            break;
        }
    }
    let reader = BufReader::new(buffer.as_slice());
    let mut lines = reader
        .lines()
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to decode log file {}", path.display()))?;
    if lines.len() > limit {
        lines = lines.split_off(lines.len() - limit);
    }
    Ok(lines)
}

fn segment_max_bytes(max_bytes: u64) -> u64 {
    DEFAULT_DAEMON_LOG_SEGMENT_MAX_BYTES.min((max_bytes / 4).max(1))
}

fn prune_target_bytes(max_bytes: u64) -> u64 {
    max_bytes.saturating_mul(3).saturating_div(4).max(1)
}

fn chrono_like_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:020}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_log_path(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "prism-cli-daemon-log-tests-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root.join("prism-mcp-daemon.log")
    }

    #[test]
    fn tail_lines_reads_recent_lines_from_archived_segments() {
        let path = temp_log_path("tail");
        for index in 0..64 {
            append_log_line_with_max_bytes(&path, &format!("line-{index}"), 256).unwrap();
        }

        let lines = tail_lines(&path, 5).unwrap();
        assert_eq!(
            lines,
            vec![
                "line-59".to_string(),
                "line-60".to_string(),
                "line-61".to_string(),
                "line-62".to_string(),
                "line-63".to_string(),
            ]
        );
    }

    #[test]
    fn append_log_line_prunes_archived_segments_to_budget() {
        let path = temp_log_path("prune");
        let max_bytes = 1_600;
        for index in 0..2_000 {
            append_log_line_with_max_bytes(
                &path,
                &format!("line-{index}-{}", "x".repeat(64)),
                max_bytes,
            )
            .unwrap();
        }

        assert!(total_log_bytes(&path).unwrap() <= max_bytes);
        assert!(!archived_segments(&path).unwrap().is_empty());
    }
}
