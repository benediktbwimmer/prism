use std::hash::{Hash, Hasher};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use prism_ir::{EdgeKind, EdgeOrigin, Language, NodeKind};
use prism_parser::NodeFingerprint;

pub(super) fn deserialize_fingerprint(raw: &str) -> NodeFingerprint {
    serde_json::from_str(raw).unwrap_or_else(|_| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        raw.hash(&mut hasher);
        NodeFingerprint::new(hasher.finish())
    })
}

pub(super) fn encode_node_kind(kind: NodeKind) -> i64 {
    match kind {
        NodeKind::Workspace => 0,
        NodeKind::Package => 1,
        NodeKind::Document => 2,
        NodeKind::Module => 3,
        NodeKind::Function => 4,
        NodeKind::Struct => 5,
        NodeKind::Enum => 6,
        NodeKind::Trait => 7,
        NodeKind::Impl => 8,
        NodeKind::Method => 9,
        NodeKind::Field => 10,
        NodeKind::TypeAlias => 11,
        NodeKind::MarkdownHeading => 12,
        NodeKind::JsonKey => 13,
        NodeKind::YamlKey => 14,
    }
}

pub(super) fn decode_node_kind(value: i64) -> rusqlite::Result<NodeKind> {
    Ok(match value {
        0 => NodeKind::Workspace,
        1 => NodeKind::Package,
        2 => NodeKind::Document,
        3 => NodeKind::Module,
        4 => NodeKind::Function,
        5 => NodeKind::Struct,
        6 => NodeKind::Enum,
        7 => NodeKind::Trait,
        8 => NodeKind::Impl,
        9 => NodeKind::Method,
        10 => NodeKind::Field,
        11 => NodeKind::TypeAlias,
        12 => NodeKind::MarkdownHeading,
        13 => NodeKind::JsonKey,
        14 => NodeKind::YamlKey,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid node kind: {other}"
            )))
        }
    })
}

pub(super) fn encode_edge_kind(kind: EdgeKind) -> i64 {
    match kind {
        EdgeKind::Contains => 0,
        EdgeKind::Calls => 1,
        EdgeKind::References => 2,
        EdgeKind::Implements => 3,
        EdgeKind::Defines => 4,
        EdgeKind::Imports => 5,
        EdgeKind::DependsOn => 6,
    }
}

pub(super) fn decode_edge_kind(value: i64) -> rusqlite::Result<EdgeKind> {
    Ok(match value {
        0 => EdgeKind::Contains,
        1 => EdgeKind::Calls,
        2 => EdgeKind::References,
        3 => EdgeKind::Implements,
        4 => EdgeKind::Defines,
        5 => EdgeKind::Imports,
        6 => EdgeKind::DependsOn,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid edge kind: {other}"
            )))
        }
    })
}

pub(super) fn encode_language(language: Language) -> i64 {
    match language {
        Language::Rust => 0,
        Language::Markdown => 1,
        Language::Json => 2,
        Language::Yaml => 3,
        Language::Unknown => 4,
    }
}

pub(super) fn decode_language(value: i64) -> rusqlite::Result<Language> {
    Ok(match value {
        0 => Language::Rust,
        1 => Language::Markdown,
        2 => Language::Json,
        3 => Language::Yaml,
        4 => Language::Unknown,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid language: {other}"
            )))
        }
    })
}

pub(super) fn encode_edge_origin(origin: EdgeOrigin) -> i64 {
    match origin {
        EdgeOrigin::Static => 0,
        EdgeOrigin::Inferred => 1,
    }
}

pub(super) fn decode_edge_origin(value: i64) -> rusqlite::Result<EdgeOrigin> {
    Ok(match value {
        0 => EdgeOrigin::Static,
        1 => EdgeOrigin::Inferred,
        other => {
            return Err(from_sql_conversion_error(format!(
                "invalid edge origin: {other}"
            )))
        }
    })
}

fn from_sql_conversion_error(message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Integer,
        Box::new(IoError::new(IoErrorKind::InvalidData, message)),
    )
}
