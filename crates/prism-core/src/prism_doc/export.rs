use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::Builder as TarBuilder;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::{PrismDocFileSync, PrismDocSyncResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrismDocBundleFormat {
    Zip,
    TarGz,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrismDocExportBundle {
    pub format: PrismDocBundleFormat,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrismDocExportResult {
    pub sync: PrismDocSyncResult,
    pub bundle: Option<PrismDocExportBundle>,
}

pub(super) fn write_bundle(
    output_root: &Path,
    files: &[PrismDocFileSync],
    format: PrismDocBundleFormat,
) -> Result<PrismDocExportBundle> {
    let bundle_path = bundle_path(output_root, format)?;
    if let Some(parent) = bundle_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match format {
        PrismDocBundleFormat::Zip => write_zip_bundle(&bundle_path, output_root, files)?,
        PrismDocBundleFormat::TarGz => write_tar_gz_bundle(&bundle_path, output_root, files)?,
    }
    Ok(PrismDocExportBundle {
        format,
        path: bundle_path,
    })
}

fn bundle_path(output_root: &Path, format: PrismDocBundleFormat) -> Result<PathBuf> {
    let file_name = output_root.file_name().with_context(|| {
        format!(
            "output directory `{}` must include a final path component",
            output_root.display()
        )
    })?;
    let parent = output_root.parent().unwrap_or_else(|| Path::new("."));
    let mut name = file_name.to_string_lossy().into_owned();
    match format {
        PrismDocBundleFormat::Zip => name.push_str(".zip"),
        PrismDocBundleFormat::TarGz => name.push_str(".tar.gz"),
    }
    Ok(parent.join(name))
}

fn write_zip_bundle(
    bundle_path: &Path,
    output_root: &Path,
    files: &[PrismDocFileSync],
) -> Result<()> {
    let writer = File::create(bundle_path)
        .with_context(|| format!("failed to create {}", bundle_path.display()))?;
    let mut zip = ZipWriter::new(writer);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    for (absolute, relative) in sorted_relative_files(output_root, files)? {
        zip.start_file(relative, options.clone())?;
        let mut file = File::open(&absolute)
            .with_context(|| format!("failed to read {}", absolute.display()))?;
        io::copy(&mut file, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

fn write_tar_gz_bundle(
    bundle_path: &Path,
    output_root: &Path,
    files: &[PrismDocFileSync],
) -> Result<()> {
    let writer = File::create(bundle_path)
        .with_context(|| format!("failed to create {}", bundle_path.display()))?;
    let encoder = GzEncoder::new(writer, Compression::default());
    let mut tar = TarBuilder::new(encoder);
    for (absolute, relative) in sorted_relative_files(output_root, files)? {
        tar.append_path_with_name(&absolute, &relative)
            .with_context(|| format!("failed to archive {}", absolute.display()))?;
    }
    tar.finish()?;
    let mut encoder = tar.into_inner()?;
    encoder.flush()?;
    let _ = encoder.finish()?;
    Ok(())
}

fn sorted_relative_files(
    output_root: &Path,
    files: &[PrismDocFileSync],
) -> Result<Vec<(PathBuf, String)>> {
    let mut entries = files
        .iter()
        .map(|file| {
            let relative = file
                .path
                .strip_prefix(output_root)
                .with_context(|| {
                    format!(
                        "exported file `{}` must stay under output root `{}`",
                        file.path.display(),
                        output_root.display()
                    )
                })?
                .to_string_lossy()
                .replace('\\', "/");
            Ok((file.path.clone(), relative))
        })
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(entries)
}
