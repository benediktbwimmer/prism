use std::path::Path;

use prism_ir::{FileId, NodeKind};
use prism_parser::{LanguageAdapter, ParseDepth, ParseInput};

use crate::RustAdapter;

#[test]
fn parses_top_level_function_and_call() {
    let adapter = RustAdapter;
    let input = ParseInput {
        package_name: "demo",
        crate_name: "demo",
        package_root: Path::new("workspace"),
        path: Path::new("workspace/src/lib.rs"),
        file_id: FileId(1),
        parse_depth: ParseDepth::Deep,
        source: "fn alpha() { beta(); }\nfn beta() {}\n",
    };

    let result = adapter.parse(&input).unwrap();
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Function && node.id.path == "demo::alpha"));
    assert!(result
        .unresolved_calls
        .iter()
        .any(|call| call.caller.path == "demo::alpha" && call.name == "beta"));
    assert!(!result
        .unresolved_calls
        .iter()
        .any(|call| call.name == "alpha"));
}

#[test]
fn parses_impls_nested_modules_and_fields() {
    let adapter = RustAdapter;
    let input = ParseInput {
        package_name: "demo",
        crate_name: "demo",
        package_root: Path::new("workspace"),
        path: Path::new("workspace/src/lib.rs"),
        file_id: FileId(1),
        parse_depth: ParseDepth::Deep,
        source: r#"
struct Config {
    value: usize,
}

trait Service {
    fn run(&self) -> usize;
}

impl Service for Config {
    fn run(&self) -> usize {
        helper()
    }
}

fn helper() -> usize { 1 }

mod nested {
    fn ping() {}
}
"#,
    };

    let result = adapter.parse(&input).unwrap();
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Field && node.id.path == "demo::Config::value"));
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Impl && node.id.path == "demo::Config::impl::Service"));
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Method && node.id.path == "demo::Config::run"));
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Module && node.id.path == "demo::nested"));
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Function && node.id.path == "demo::nested::ping"));
    assert!(result
        .unresolved_calls
        .iter()
        .any(|call| call.caller.path == "demo::Config::run" && call.name == "helper"));
}

#[test]
fn collects_imports_and_trait_references() {
    let adapter = RustAdapter;
    let input = ParseInput {
        package_name: "demo",
        crate_name: "demo",
        package_root: Path::new("workspace"),
        path: Path::new("workspace/src/lib.rs"),
        file_id: FileId(1),
        parse_depth: ParseDepth::Deep,
        source: r#"
use crate::net::Client;
use self::models::{User as AppUser, Account};
use super::shared::Thing;

trait Runner {}
struct Job;

impl Runner for Job {}
"#,
    };

    let result = adapter.parse(&input).unwrap();
    assert!(result
        .unresolved_imports
        .iter()
        .any(|import| import.path == "demo::net::Client"));
    assert!(result
        .unresolved_imports
        .iter()
        .any(|import| import.path == "demo::models::User"));
    assert!(result
        .unresolved_imports
        .iter()
        .any(|import| import.path == "demo::models::Account"));
    assert!(result
        .unresolved_impls
        .iter()
        .any(|implementation| implementation.target == "demo::Runner"));
}

#[test]
fn shallow_parse_skips_body_derived_calls() {
    let adapter = RustAdapter;
    let input = ParseInput {
        package_name: "demo",
        crate_name: "demo",
        package_root: Path::new("workspace"),
        path: Path::new("workspace/src/lib.rs"),
        file_id: FileId(1),
        parse_depth: ParseDepth::Shallow,
        source: "fn alpha() { beta(); }\nfn beta() {}\n",
    };

    let result = adapter.parse(&input).unwrap();
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Function && node.id.path == "demo::alpha"));
    assert!(result.unresolved_calls.is_empty());
}
