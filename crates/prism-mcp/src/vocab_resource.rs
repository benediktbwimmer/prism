use crate::{
    capabilities_resource_view_link, dedupe_resource_link_views, resource_link_view,
    schema_resource_uri, schema_resource_view_link, session_resource_view_link,
    tool_schemas_resource_view_link, vocab_resource_uri, vocab_resource_view_link,
    vocabulary_categories, PrismMcpFeatures, VocabularyCategoryView, VocabularyResourcePayload,
    VocabularyValueView, VOCAB_URI,
};

fn vocabulary_category_view(
    category: &crate::VocabularyCategorySpec,
    features: &PrismMcpFeatures,
) -> Option<VocabularyCategoryView> {
    if !features.vocabulary_category_visible(category.key) {
        return None;
    }
    let values = category
        .values
        .iter()
        .filter(|value| features.vocabulary_value_visible(category.key, value.value))
        .map(|value| VocabularyValueView {
            value: value.value.to_string(),
            aliases: value
                .aliases
                .iter()
                .map(|alias| (*alias).to_string())
                .collect(),
            description: value.description.to_string(),
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    Some(VocabularyCategoryView {
        key: category.key.to_string(),
        title: category.title.to_string(),
        description: category.description.to_string(),
        values,
    })
}

pub(crate) fn vocab_resource_value(features: &PrismMcpFeatures) -> VocabularyResourcePayload {
    VocabularyResourcePayload {
        uri: vocab_resource_uri(),
        schema_uri: schema_resource_uri("vocab"),
        vocabularies: vocabulary_categories()
            .iter()
            .filter_map(|category| vocabulary_category_view(category, features))
            .collect(),
        related_resources: dedupe_resource_link_views(vec![
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
        ]),
    }
}
