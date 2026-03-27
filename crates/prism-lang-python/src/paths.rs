use prism_parser::{relative_package_file, ParseInput};

pub(crate) fn module_path(input: &ParseInput<'_>) -> String {
    let relative = relative_package_file(input);
    let relative = relative.strip_prefix("src").unwrap_or(relative.as_path());
    let mut suffix = relative
        .parent()
        .into_iter()
        .flat_map(|path| path.components())
        .map(|component| normalize_path_segment(&component.as_os_str().to_string_lossy()))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    let file_stem = relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if file_stem != "__init__" && !file_stem.is_empty() {
        let normalized = normalize_path_segment(file_stem);
        if !normalized.is_empty() {
            suffix.push(normalized);
        }
    }

    if suffix
        .first()
        .is_some_and(|segment| segment == input.crate_name)
    {
        suffix.remove(0);
    }

    let mut parts = vec![input.crate_name.to_owned()];
    parts.extend(suffix);
    parts.join("::")
}

pub(crate) fn is_package_init(input: &ParseInput<'_>) -> bool {
    input
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "__init__")
}

pub(crate) fn absolute_symbol_path(value: &str, crate_name: &str) -> String {
    let mut segments = split_symbol_segments(value);
    if segments.is_empty() {
        return crate_name.to_owned();
    }
    if segments
        .first()
        .is_some_and(|segment| segment == crate_name)
    {
        return segments.join("::");
    }

    let mut parts = vec![crate_name.to_owned()];
    parts.append(&mut segments);
    parts.join("::")
}

pub(crate) fn import_path(
    module_text: Option<&str>,
    imported_name: Option<&str>,
    level: usize,
    current_module_path: &str,
    crate_name: &str,
    current_is_package: bool,
) -> String {
    let mut parts = if level == 0 {
        module_text
            .map(|text| absolute_symbol_path(text, crate_name))
            .unwrap_or_else(|| crate_name.to_owned())
            .split("::")
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    } else {
        relative_base_parts(current_module_path, level, current_is_package)
    };

    if level > 0 {
        if let Some(module_text) = module_text {
            parts.extend(split_symbol_segments(module_text));
        }
    }

    if let Some(imported_name) = imported_name {
        parts.extend(split_symbol_segments(imported_name));
    }

    parts.join("::")
}

pub(crate) fn split_relative_module_spec(value: &str) -> (usize, Option<&str>) {
    let trimmed = value.trim();
    let level = trimmed.chars().take_while(|ch| *ch == '.').count();
    let remainder = trimmed[level..].trim();
    let remainder = (!remainder.is_empty()).then_some(remainder);
    (level, remainder)
}

pub(crate) fn dotted_reference_target(value: &str, crate_name: &str) -> String {
    if value.contains('.') {
        absolute_symbol_path(value, crate_name)
    } else {
        simplify_symbol(value)
    }
}

pub(crate) fn simplify_symbol(value: &str) -> String {
    value
        .rsplit("::")
        .next()
        .unwrap_or(value)
        .rsplit('.')
        .next()
        .unwrap_or(value)
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .to_owned()
}

fn relative_base_parts(
    current_module_path: &str,
    level: usize,
    current_is_package: bool,
) -> Vec<String> {
    let mut parts = current_module_path
        .split("::")
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if !current_is_package && parts.len() > 1 {
        parts.pop();
    }
    for _ in 0..level.saturating_sub(1) {
        if parts.len() > 1 {
            parts.pop();
        }
    }
    parts
}

fn split_symbol_segments(value: &str) -> Vec<String> {
    value
        .split(['.', ':'])
        .filter(|segment| !segment.is_empty())
        .map(normalize_path_segment)
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn normalize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch == '-' { '_' } else { ch })
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect()
}
