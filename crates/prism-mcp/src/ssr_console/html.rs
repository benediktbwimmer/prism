use pulldown_cmark::{html, Options, Parser};

use super::assets::{HTMX_CDN, MERMAID_CDN, VIS_TIMELINE_CSS_CDN, VIS_TIMELINE_JS_CDN};
use crate::SessionView;

pub(crate) fn page_shell(
    title: &str,
    subtitle: &str,
    active_nav: &str,
    session: &SessionView,
    body: &str,
) -> String {
    let operator = session
        .bridge_identity
        .as_ref()
        .and_then(|identity| identity.profile.clone().or(identity.principal_id.clone()))
        .unwrap_or_else(|| "local operator".to_string());
    let workspace = session
        .workspace_root
        .clone()
        .unwrap_or_else(|| "unknown workspace".to_string());
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{}</title>\
         <link rel=\"icon\" href=\"/console/favicon.svg\" type=\"image/svg+xml\">\
         <link rel=\"stylesheet\" href=\"{}\">\
         <link rel=\"stylesheet\" href=\"/console/assets/console.css\">\
         <script defer src=\"{}\"></script>\
         <script defer src=\"{}\"></script>\
         <script defer src=\"{}\"></script>\
         <script defer src=\"/console/assets/console.js\"></script>\
         </head><body>\
         <div class=\"console-shell\"><div class=\"console-frame\">\
         <header class=\"console-topbar\">\
         <div class=\"console-brand\">\
         <div><p class=\"console-eyebrow\">PRISM SSR Console</p><h1>{}</h1><p class=\"console-subtitle\">{}</p></div>\
         <div class=\"console-meta-grid\">\
         <div class=\"console-meta-card\"><span class=\"console-eyebrow\">Operator</span><strong>{}</strong></div>\
         <div class=\"console-meta-card\"><span class=\"console-eyebrow\">Workspace</span><strong>{}</strong></div>\
         </div></div>\
         <nav class=\"console-nav\">{}\
         </nav></header>{}</div></div></body></html>",
        escape_html(title),
        VIS_TIMELINE_CSS_CDN,
        HTMX_CDN,
        MERMAID_CDN,
        VIS_TIMELINE_JS_CDN,
        escape_html(title),
        escape_html(subtitle),
        escape_html(&operator),
        escape_html(&workspace),
        render_nav(active_nav),
        body
    )
}

pub(crate) fn render_nav(active_nav: &str) -> String {
    let items = [
        ("overview", "/console", "Overview"),
        ("plans", "/console/plans", "Plans"),
        ("concepts", "/console/concepts", "Concepts"),
        ("fleet", "/console/fleet", "Fleet"),
    ];
    items
        .into_iter()
        .map(|(id, href, label)| {
            format!(
                "<a href=\"{}\" data-active=\"{}\">{}</a>",
                href,
                if id == active_nav { "true" } else { "false" },
                escape_html(label)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub(crate) fn json_script_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub(crate) fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(markdown, options);
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);
    rendered
}

pub(crate) fn status_badge(status: &str) -> String {
    let slug = status_slug(status);
    format!(
        "<span class=\"console-status console-status--{}\">{}</span>",
        slug,
        escape_html(status)
    )
}

pub(crate) fn status_slug(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| match ch {
            'A'..='Z' => Some(ch.to_ascii_lowercase()),
            'a'..='z' | '0'..='9' => Some(ch),
            _ => Some('-'),
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>()
        + "…"
}

pub(crate) fn percent(value: usize, total: usize) -> usize {
    if total == 0 {
        0
    } else {
        ((value as f64 / total as f64) * 100.0).round() as usize
    }
}

pub(crate) fn duration_label(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "unknown".to_string();
    };
    if seconds >= 3600 {
        format!("{:.1}h", seconds as f64 / 3600.0)
    } else if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::json_script_escape;

    #[test]
    fn json_script_escape_preserves_json_while_escaping_script_sensitive_chars() {
        let escaped = json_script_escape(r#"{"html":"<&>","lineSep":" ","paraSep":" "}"#);
        assert!(escaped.contains(r#""html":"\u003c\u0026\u003e""#));
        assert!(escaped.contains(r#""lineSep":"\u2028""#));
        assert!(escaped.contains(r#""paraSep":"\u2029""#));
        assert!(!escaped.contains("&quot;"));
    }
}
