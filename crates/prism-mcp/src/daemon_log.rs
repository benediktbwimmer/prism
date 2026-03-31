use std::collections::VecDeque;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use prism_ir::new_sortable_token;
use tracing_subscriber::fmt::writer::MakeWriter;

const DEFAULT_DAEMON_LOG_MAX_BYTES: u64 = 64 * 1024 * 1024;
const DEFAULT_DAEMON_LOG_SEGMENT_MAX_BYTES: u64 = 8 * 1024 * 1024;
const DAEMON_LOG_MAX_BYTES_ENV: &str = "PRISM_MCP_DAEMON_LOG_MAX_BYTES";
const TAIL_READ_CHUNK_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone)]
struct SegmentFile {
    path: PathBuf,
    bytes: u64,
}

#[derive(Debug)]
struct RotatingDaemonLogState {
    path: PathBuf,
    file: File,
    max_bytes: u64,
    active_bytes: u64,
    archived_bytes: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct DaemonLogMakeWriter {
    inner: Arc<Mutex<RotatingDaemonLogState>>,
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

pub(crate) fn make_writer(path: PathBuf) -> Result<DaemonLogMakeWriter> {
    make_writer_with_max_bytes(path, configured_max_bytes())
}

fn make_writer_with_max_bytes(path: PathBuf, max_bytes: u64) -> Result<DaemonLogMakeWriter> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let archived_bytes = archived_segments(&path)?
        .into_iter()
        .fold(0u64, |acc, segment| acc.saturating_add(segment.bytes));
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open daemon log {}", path.display()))?;
    let active_bytes = file
        .metadata()
        .with_context(|| format!("failed to stat daemon log {}", path.display()))?
        .len();
    Ok(DaemonLogMakeWriter {
        inner: Arc::new(Mutex::new(RotatingDaemonLogState {
            path,
            file,
            max_bytes,
            active_bytes,
            archived_bytes,
        })),
    })
}

impl<'a> MakeWriter<'a> for DaemonLogMakeWriter {
    type Writer = DaemonLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        DaemonLogWriter {
            inner: Arc::clone(&self.inner),
        }
    }
}

pub(crate) struct DaemonLogWriter {
    inner: Arc<Mutex<RotatingDaemonLogState>>,
}

impl Write for DaemonLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.inner.lock().expect("daemon log lock poisoned");
        state.write_bytes(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner
            .lock()
            .expect("daemon log lock poisoned")
            .file
            .flush()
    }
}

impl RotatingDaemonLogState {
    fn write_bytes(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.rotate_if_needed(buf.len())?;
        self.file.write_all(buf)?;
        self.active_bytes = self.active_bytes.saturating_add(buf.len() as u64);
        if self.archived_bytes.saturating_add(self.active_bytes) > self.max_bytes {
            self.prune_archived_segments()?;
        }
        Ok(buf.len())
    }

    fn rotate_if_needed(&mut self, next_write_bytes: usize) -> io::Result<()> {
        if self.active_bytes == 0 {
            return Ok(());
        }
        if self.active_bytes.saturating_add(next_write_bytes as u64)
            <= segment_max_bytes(self.max_bytes)
        {
            return Ok(());
        }
        self.file.flush()?;
        let archive_path = archived_segment_path(&self.path, &new_sortable_token());
        fs::rename(&self.path, &archive_path)?;
        self.archived_bytes = self.archived_bytes.saturating_add(self.active_bytes);
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.active_bytes = 0;
        Ok(())
    }

    fn prune_archived_segments(&mut self) -> io::Result<()> {
        let prune_target = prune_target_bytes(self.max_bytes);
        for segment in archived_segments(&self.path).map_err(to_io_error)? {
            if self.archived_bytes.saturating_add(self.active_bytes) <= prune_target {
                break;
            }
            fs::remove_file(&segment.path)?;
            self.archived_bytes = self.archived_bytes.saturating_sub(segment.bytes);
        }
        Ok(())
    }
}

fn configured_max_bytes() -> u64 {
    env::var(DAEMON_LOG_MAX_BYTES_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_DAEMON_LOG_MAX_BYTES)
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

fn archived_segment_path(path: &Path, token: &str) -> PathBuf {
    let (prefix, suffix) = archived_segment_name_parts(path);
    path.with_file_name(format!("{prefix}{token}{suffix}"))
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

fn to_io_error(error: anyhow::Error) -> io::Error {
    io::Error::other(error.to_string())
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
            "prism-mcp-daemon-log-tests-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root.join("prism-mcp-daemon.log")
    }

    #[test]
    fn rotating_writer_tails_across_archived_segments() {
        let path = temp_log_path("tail");
        let writer = make_writer_with_max_bytes(path.clone(), 2_048).unwrap();
        for index in 0..200 {
            let mut handle = writer.make_writer();
            writeln!(handle, "line-{index}-{}", "x".repeat(80)).unwrap();
            handle.flush().unwrap();
        }

        let lines = tail_lines(&path, 4).unwrap();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("line-196-"));
        assert!(lines[3].contains("line-199-"));
    }

    #[test]
    fn rotating_writer_prunes_to_total_budget() {
        let path = temp_log_path("prune");
        let max_bytes = 1_600;
        let writer = make_writer_with_max_bytes(path.clone(), max_bytes).unwrap();
        for index in 0..2_000 {
            let mut handle = writer.make_writer();
            writeln!(handle, "line-{index}-{}", "x".repeat(80)).unwrap();
        }

        assert!(total_log_bytes(&path).unwrap() <= max_bytes);
        assert!(!archived_segments(&path).unwrap().is_empty());
    }
}
