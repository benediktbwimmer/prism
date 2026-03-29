use smol_str::SmolStr;
use ulid::Ulid;

pub fn new_prefixed_id(prefix: &str) -> SmolStr {
    format!("{prefix}:{}", new_sortable_token()).into()
}

pub fn new_slugged_id(prefix: &str, value: &str) -> SmolStr {
    let slug = slugify_id_fragment(value);
    if slug.is_empty() {
        new_prefixed_id(prefix)
    } else {
        format!("{prefix}:{slug}:{}", new_sortable_token()).into()
    }
}

pub fn new_sortable_token() -> SmolStr {
    Ulid::new().to_string().to_ascii_lowercase().into()
}

pub fn slugify_id_fragment(value: &str) -> String {
    let mut slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug.trim_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::{new_prefixed_id, new_slugged_id, slugify_id_fragment};

    #[test]
    fn prefixed_ids_keep_prefix_and_sortable_token() {
        let value = new_prefixed_id("plan");
        assert!(value.starts_with("plan:"));
        assert_eq!(value.matches(':').count(), 1);
    }

    #[test]
    fn slugged_ids_normalize_human_text() {
        let value = new_slugged_id("task", "Fix merge-unsafe IDs");
        assert!(value.starts_with("task:fix-merge-unsafe-ids:"));
    }

    #[test]
    fn slugify_collapses_repeated_separators() {
        assert_eq!(slugify_id_fragment("  alpha  / beta  "), "alpha-beta");
    }
}
