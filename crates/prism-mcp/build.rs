use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dist_dir = manifest_dir.join("../../www/dashboard/dist");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let generated = out_dir.join("prism_ui_assets_generated.rs");

    println!("cargo:rerun-if-changed={}", dist_dir.display());

    let source = match collect_dist_files(&dist_dir) {
        Ok(files) => generate_assets_module(&files),
        Err(_) => generate_assets_module(&[]),
    };

    fs::write(&generated, source).expect("write generated ui asset module");
}

fn collect_dist_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    collect_files_recursive(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_recursive(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(root, &path, files)?;
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        files.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
    }
    Ok(())
}

fn generate_assets_module(files: &[PathBuf]) -> String {
    let mut out = String::new();
    out.push_str(
        "pub(crate) fn embedded_prism_ui_asset(path: &str) -> Option<EmbeddedUiAsset> {\n",
    );
    out.push_str("    match path {\n");
    for relative in files {
        let key = normalize_path(relative);
        let abs = canonicalize_for_include(relative);
        let mime = mime_for_path(&key);
        out.push_str(&format!(
            "        {key:?} => Some(EmbeddedUiAsset {{ bytes: include_bytes!(r#\"{abs}\"#), mime: {mime:?} }}),\n"
        ));
    }
    out.push_str("        _ => None,\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

fn canonicalize_for_include(relative: &Path) -> String {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    manifest_dir
        .join("../../www/dashboard/dist")
        .join(relative)
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../../www/dashboard/dist").join(relative))
        .display()
        .to_string()
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn mime_for_path(path: &str) -> &'static str {
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
