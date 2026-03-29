use crate::{
    capabilities_resource_view_link, dedupe_resource_link_views, resource_link_view,
    schema_resource_uri, schema_resource_view_link, session_resource_view_link,
    tool_schemas_resource_view_link, vocab_resource_uri, vocab_resource_view_link,
    vocabulary_categories, VocabularyCategoryView, VocabularyResourcePayload, VocabularyValueView,
    VOCAB_URI,
};

fn vocabulary_category_view(category: &crate::VocabularyCategorySpec) -> VocabularyCategoryView {
    VocabularyCategoryView {
        key: category.key.to_string(),
        title: category.title.to_string(),
        description: category.description.to_string(),
        values: category
            .values
            .iter()
            .map(|value| VocabularyValueView {
                value: value.value.to_string(),
                aliases: value
                    .aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect(),
                description: value.description.to_string(),
            })
            .collect(),
    }
}

pub(crate) fn vocab_resource_value() -> VocabularyResourcePayload {
    let related_resources = dedupe_resource_link_views(vec![
        vocab_resource_view_link(),
        capabilities_resource_view_link(),
        tool_schemas_resource_view_link(),
        session_resource_view_link(),
        schema_resource_view_link("vocab"),
        resource_link_view(
            VOCAB_URI.to_string(),
            "PRISM Vocabulary",
            "Canonical enum and action vocabularies for PRISM MCP resources, query args, and mutation payloads.",
        ),
    ]);
    VocabularyResourcePayload {
        uri: vocab_resource_uri(),
        schema_uri: schema_resource_uri("vocab"),
        vocabularies: vocabulary_categories()
            .iter()
            .map(vocabulary_category_view)
            .collect(),
        related_resources,
    }
}
