use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(crate) fn prism_ui_root(root: &Path) -> PathBuf {
    root.join("www").join("dashboard")
}

pub(crate) fn prism_ui_dist_dir(root: &Path) -> PathBuf {
    prism_ui_root(root).join("dist")
}

pub(crate) fn prism_ui_index_html(root: &Path) -> Result<Option<String>> {
    let path = prism_ui_dist_dir(root).join("index.html");
    if !path.exists() {
        return Ok(None);
    }
    let html = fs::read_to_string(&path)
        .with_context(|| format!("failed to read prism ui index {}", path.display()))?;
    Ok(Some(html))
}

pub(crate) fn prism_ui_unbuilt_html(root: &Path) -> String {
    let app_root = prism_ui_root(root);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>PRISM UI</title><style>body{{font-family:ui-sans-serif,system-ui,sans-serif;margin:0;background:#111827;color:#f9fafb}}main{{max-width:840px;margin:0 auto;padding:48px 24px}}code{{background:#1f2937;padding:2px 6px;border-radius:6px}}pre{{background:#0f172a;padding:16px;border-radius:12px;overflow:auto}}a{{color:#93c5fd}}</style></head><body><main><h1>PRISM UI</h1><p>The frontend source exists, but built assets were not found yet.</p><p>Build it from <code>{}</code> with:</p><pre>npm install\nnpm run build</pre><p>The web app is served at <code>/</code>, <code>/plans</code>, and <code>/graph</code>.</p></main></body></html>",
        app_root.display()
    )
}
