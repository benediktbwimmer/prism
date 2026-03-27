use std::path::Path;

use prism_ir::{EdgeKind, FileId, NodeKind};
use prism_parser::{LanguageAdapter, ParseInput};

use crate::PythonAdapter;

#[test]
fn parses_top_level_function_and_call() {
    let adapter = PythonAdapter;
    let input = ParseInput {
        package_name: "demo",
        crate_name: "demo",
        package_root: Path::new("/workspace"),
        path: Path::new("/workspace/demo/service.py"),
        file_id: FileId(1),
        source: "def alpha():\n    beta()\n\ndef beta():\n    pass\n",
    };

    let result = adapter.parse(&input).unwrap();
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Function && node.id.path == "demo::service::alpha"));
    assert!(result
        .unresolved_calls
        .iter()
        .any(|call| call.caller.path == "demo::service::alpha" && call.name == "beta"));
}

#[test]
fn parses_classes_methods_and_fields() {
    let adapter = PythonAdapter;
    let input = ParseInput {
        package_name: "demo",
        crate_name: "demo",
        package_root: Path::new("/workspace"),
        path: Path::new("/workspace/demo/__init__.py"),
        file_id: FileId(1),
        source: r#"
class Config:
    setting = 1

    def __init__(self):
        self.value = helper()


def helper():
    return 1
"#,
    };

    let result = adapter.parse(&input).unwrap();
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Struct && node.id.path == "demo::Config"));
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Field && node.id.path == "demo::Config::setting"));
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Field && node.id.path == "demo::Config::value"));
    assert!(result
        .nodes
        .iter()
        .any(|node| node.kind == NodeKind::Method && node.id.path == "demo::Config::__init__"));
    assert!(result
        .unresolved_calls
        .iter()
        .any(|call| call.caller.path == "demo::Config::__init__" && call.name == "helper"));
}

#[test]
fn collects_imports_and_inheritance_references() {
    let adapter = PythonAdapter;
    let input = ParseInput {
        package_name: "demo",
        crate_name: "demo",
        package_root: Path::new("/workspace"),
        path: Path::new("/workspace/demo/models.py"),
        file_id: FileId(1),
        source: r#"
from .base import Base
import demo.helpers as helpers


class Item(Base):
    pass
"#,
    };

    let result = adapter.parse(&input).unwrap();
    assert!(result
        .unresolved_imports
        .iter()
        .any(|import| import.path == "demo::base::Base"));
    assert!(result
        .unresolved_imports
        .iter()
        .any(|import| import.path == "demo::helpers"));
    assert!(result.unresolved_intents.iter().any(|intent| {
        intent.kind == EdgeKind::RelatedTo
            && intent.source.path == "demo::models::Item"
            && intent.target == "Base"
    }));
}
