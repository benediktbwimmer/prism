use std::thread;
use std::time::Instant;

use anyhow::{anyhow, Result};
use prism_ir::{FileId, Language};
use prism_parser::{LanguageAdapter, ParseDepth, ParseInput, ParseResult};

use crate::layout::PackageInfo;
use crate::PendingFileParse;

#[derive(Debug, Clone)]
pub(crate) struct PreparedParseJob {
    pub(crate) pending: PendingFileParse,
    pub(crate) file_id: FileId,
    pub(crate) package: PackageInfo,
    pub(crate) adapter_index: usize,
    pub(crate) language: Language,
    pub(crate) parse_depth: ParseDepth,
}

#[derive(Debug)]
pub(crate) struct ParsedParseJob {
    pub(crate) pending: PendingFileParse,
    pub(crate) package: PackageInfo,
    pub(crate) parsed: ParseResult,
    pub(crate) language: Language,
    pub(crate) parse_depth: ParseDepth,
    pub(crate) parse_ms: u128,
}

#[derive(Debug)]
pub(crate) struct ParsedJobBatch {
    pub(crate) jobs: Vec<ParsedParseJob>,
    pub(crate) worker_count: usize,
}

pub(crate) fn parse_jobs_in_parallel(
    adapters: &[Box<dyn LanguageAdapter + Send + Sync>],
    jobs: Vec<PreparedParseJob>,
) -> Result<ParsedJobBatch> {
    if jobs.is_empty() {
        return Ok(ParsedJobBatch {
            jobs: Vec::new(),
            worker_count: 0,
        });
    }

    let worker_count = parse_worker_count(jobs.len());
    if worker_count <= 1 {
        return Ok(ParsedJobBatch {
            jobs: parse_job_chunk(adapters, jobs)?,
            worker_count: 1,
        });
    }

    let job_chunks = chunk_jobs(jobs, worker_count);
    let mut parsed_jobs = Vec::new();
    thread::scope(|scope| -> Result<()> {
        let handles = job_chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || parse_job_chunk(adapters, chunk)))
            .collect::<Vec<_>>();
        for handle in handles {
            let mut chunk = handle
                .join()
                .map_err(|payload| anyhow!("parallel parse worker panicked: {payload:?}"))??;
            parsed_jobs.append(&mut chunk);
        }
        Ok(())
    })?;

    Ok(ParsedJobBatch {
        jobs: parsed_jobs,
        worker_count,
    })
}

fn parse_worker_count(job_count: usize) -> usize {
    thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(job_count)
}

fn chunk_jobs(mut jobs: Vec<PreparedParseJob>, worker_count: usize) -> Vec<Vec<PreparedParseJob>> {
    let chunk_size = jobs.len().div_ceil(worker_count);
    let mut chunks = Vec::new();
    while !jobs.is_empty() {
        if jobs.len() <= chunk_size {
            chunks.push(jobs);
            break;
        }
        let tail = jobs.split_off(chunk_size);
        chunks.push(jobs);
        jobs = tail;
    }
    chunks
}

fn parse_job_chunk(
    adapters: &[Box<dyn LanguageAdapter + Send + Sync>],
    jobs: Vec<PreparedParseJob>,
) -> Result<Vec<ParsedParseJob>> {
    let mut parsed_jobs = Vec::with_capacity(jobs.len());
    for job in jobs {
        let input = ParseInput {
            package_name: &job.package.package_name,
            crate_name: &job.package.crate_name,
            package_root: &job.package.root,
            path: &job.pending.path,
            file_id: job.file_id,
            parse_depth: job.parse_depth,
            source: &job.pending.source,
        };
        let parse_started = Instant::now();
        let parsed = adapters[job.adapter_index].parse(&input)?;
        parsed_jobs.push(ParsedParseJob {
            pending: job.pending,
            package: job.package,
            parsed,
            language: job.language,
            parse_depth: job.parse_depth,
            parse_ms: parse_started.elapsed().as_millis(),
        });
    }
    Ok(parsed_jobs)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use anyhow::Result;
    use prism_ir::{FileId, Language};
    use prism_parser::{LanguageAdapter, ParseDepth, ParseInput, ParseResult};

    use super::{parse_jobs_in_parallel, PreparedParseJob};
    use crate::layout::PackageInfo;
    use crate::PendingFileParse;

    struct SlowTestAdapter {
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    fn temp_workspace_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "prism-parse-pipeline-tests-{}-{stamp}",
            std::process::id()
        ))
    }

    impl LanguageAdapter for SlowTestAdapter {
        fn language(&self) -> Language {
            Language::Rust
        }

        fn supports_path(&self, _path: &Path) -> bool {
            true
        }

        fn parse(&self, _input: &ParseInput<'_>) -> Result<ParseResult> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(20));
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(ParseResult::default())
        }
    }

    #[test]
    fn parallel_parse_preserves_order_and_uses_multiple_workers_when_available() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let adapters: Vec<Box<dyn LanguageAdapter + Send + Sync>> =
            vec![Box::new(SlowTestAdapter {
                active: Arc::clone(&active),
                max_active: Arc::clone(&max_active),
            })];
        let root = temp_workspace_root();
        let package = PackageInfo::new(
            "prism-core".to_owned(),
            root.clone(),
            root.join("Cargo.toml"),
        );
        let jobs = (0..4)
            .map(|index| PreparedParseJob {
                pending: PendingFileParse {
                    path: root.join(format!("src/file_{index}.rs")),
                    source: format!("fn file_{index}() {{}}\n"),
                    hash: index,
                    previous_path: None,
                },
                file_id: FileId(index as u32 + 1),
                package: package.clone(),
                adapter_index: 0,
                language: Language::Rust,
                parse_depth: ParseDepth::Deep,
            })
            .collect::<Vec<_>>();

        let parsed = parse_jobs_in_parallel(&adapters, jobs).expect("parse jobs succeed");
        let parsed_paths = parsed
            .jobs
            .iter()
            .map(|job| job.pending.path.display().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            parsed_paths,
            (0..4)
                .map(|index| root.join(format!("src/file_{index}.rs")).display().to_string())
                .collect::<Vec<_>>()
        );
        if parsed.worker_count > 1 {
            assert!(max_active.load(Ordering::SeqCst) > 1);
        }
    }
}
