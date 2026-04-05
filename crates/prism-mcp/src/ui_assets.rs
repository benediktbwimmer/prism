use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(crate) struct EmbeddedUiAsset {
    pub(crate) bytes: &'static [u8],
    pub(crate) mime: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/prism_ui_assets_generated.rs"));

pub(crate) fn prism_ui_root(root: &Path) -> PathBuf {
    root.join("www").join("dashboard")
}

pub(crate) fn prism_ui_dist_dir(root: &Path) -> PathBuf {
    prism_ui_root(root).join("dist")
}

pub(crate) fn prism_ui_index_html(root: &Path) -> Result<Option<String>> {
    if let Some(asset) = embedded_prism_ui_asset("index.html") {
        let html = std::str::from_utf8(asset.bytes)
            .context("embedded prism ui index is not valid utf-8")?
            .to_owned();
        return Ok(Some(html));
    }

    let path = prism_ui_dist_dir(root).join("index.html");
    if !path.exists() {
        return Ok(None);
    }
    let html = fs::read_to_string(&path)
        .with_context(|| format!("failed to read prism ui index {}", path.display()))?;
    Ok(Some(html))
}

pub(crate) fn prism_ui_asset(root: &Path, path: &str) -> Result<Option<(Vec<u8>, &'static str)>> {
    let normalized = normalize_ui_asset_path(path);
    if let Some(asset) = embedded_prism_ui_asset(&normalized) {
        return Ok(Some((asset.bytes.to_vec(), asset.mime)));
    }

    let disk_path = prism_ui_dist_dir(root).join(&normalized);
    if !disk_path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&disk_path)
        .with_context(|| format!("failed to read prism ui asset {}", disk_path.display()))?;
    Ok(Some((bytes, disk_mime_for_path(&normalized))))
}

pub(crate) fn prism_ui_unbuilt_html(root: &Path) -> String {
    let app_root = prism_ui_root(root);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>PRISM UI</title><style>body{{font-family:ui-sans-serif,system-ui,sans-serif;margin:0;background:#111827;color:#f9fafb}}main{{max-width:840px;margin:0 auto;padding:48px 24px}}code{{background:#1f2937;padding:2px 6px;border-radius:6px}}pre{{background:#0f172a;padding:16px;border-radius:12px;overflow:auto}}a{{color:#93c5fd}}</style></head><body><main><h1>PRISM UI</h1><p>The frontend source exists, but built assets were not embedded into this binary and were not found on disk.</p><p>Build the dashboard from <code>{}</code> with:</p><pre>npm install\nnpm run build</pre><p>The app shell is served at <code>/</code>, <code>/plans</code>, <code>/graph</code>, and <code>/fleet</code>.</p></main></body></html>",
        app_root.display()
    )
}

fn normalize_ui_asset_path(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

fn disk_mime_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        Some("map") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}
