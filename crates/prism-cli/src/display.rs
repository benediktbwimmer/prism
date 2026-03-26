use prism_ir::NodeId;
use prism_memory::ScoredMemory;
use prism_query::{Relations, Symbol};

pub fn print_symbol(symbol: Symbol<'_>) {
    println!("{}", symbol.signature());
    let full = symbol.full();
    if !full.trim().is_empty() {
        println!("{full}");
    }
    print_relation_section("calls", &symbol.skeleton().calls);
    print_relation_section("imports", &symbol.imports());
    print_relation_section("implements", &symbol.implements());
    print_relation_section("called by", &symbol.callers());
    print_relation_section("imported by", &symbol.imported_by());
    print_relation_section("implemented by", &symbol.implemented_by());
}

pub fn print_relations(symbol: Symbol<'_>) {
    println!("{}", symbol.signature());
    let relations = symbol.relations();
    print_named_relations(relations);
}

pub fn print_lineage(prism: &prism_query::Prism, symbol: Symbol<'_>) {
    println!("{}", symbol.signature());
    let Some(lineage) = prism.lineage_of(symbol.id()) else {
        println!("no lineage");
        return;
    };
    println!("lineage: {}", lineage.0);
    for event in prism.lineage_history(&lineage) {
        let before = event
            .before
            .iter()
            .map(|node| node.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let after = event
            .after
            .iter()
            .map(|node| node.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!("  {:?}: [{}] -> [{}]", event.kind, before, after);
    }
}

pub fn print_named_relations(relations: Relations) {
    print_relation_section("calls", &relations.outgoing_calls);
    print_relation_section("called by", &relations.incoming_calls);
    print_relation_section("imports", &relations.outgoing_imports);
    print_relation_section("imported by", &relations.incoming_imports);
    print_relation_section("implements", &relations.outgoing_implements);
    print_relation_section("implemented by", &relations.incoming_implements);
}

pub fn print_relation_section(label: &str, values: &[NodeId]) {
    if values.is_empty() {
        return;
    }
    println!("{label}:");
    for value in values {
        println!("  {}", value.path);
    }
}

pub fn print_scored_memory(memory: ScoredMemory) {
    println!(
        "  [{}] score={:.2} source={} trust={:.2} created_at={}",
        memory.id.0,
        memory.score,
        format!("{:?}", memory.entry.source),
        memory.entry.trust,
        memory.entry.created_at
    );
    println!("    {}", memory.entry.content);
    if let Some(explanation) = memory.explanation {
        println!("    explanation: {explanation}");
    }
}
