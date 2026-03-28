use crate::text::{cosine_similarity, hashed_embedding};

use super::config::{SemanticBackendKind, SemanticMemoryConfig};
use super::SemanticCandidate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "openai-embeddings"), allow(dead_code))]
pub(crate) enum SemanticSignalSource {
    Local,
    OpenAi,
    LocalFallback,
}

impl SemanticSignalSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::OpenAi => "openai",
            Self::LocalFallback => "local-fallback",
        }
    }
}

pub(crate) enum SemanticBackendRuntime {
    Local,
    #[cfg(feature = "openai-embeddings")]
    OpenAi(super::openai::OpenAiEmbeddingBackend),
    OpenAiUnavailable,
}

impl SemanticBackendRuntime {
    pub(crate) fn new(config: &SemanticMemoryConfig) -> Self {
        match config.preferred_backend {
            SemanticBackendKind::Local => Self::Local,
            SemanticBackendKind::OpenAi => {
                #[cfg(feature = "openai-embeddings")]
                {
                    if let Some(openai) = &config.openai {
                        if let Ok(backend) = super::openai::OpenAiEmbeddingBackend::new(openai) {
                            return Self::OpenAi(backend);
                        }
                    }
                }
                Self::OpenAiUnavailable
            }
        }
    }

    pub(crate) fn refresh_semantic_scores(
        &self,
        query_text: &str,
        candidates: &mut [SemanticCandidate],
        limit: usize,
    ) {
        if query_text.trim().is_empty() || candidates.is_empty() {
            return;
        }

        let query_embedding = hashed_embedding(query_text);
        for candidate in candidates.iter_mut() {
            candidate.semantic =
                cosine_similarity(&hashed_embedding(&candidate.text), &query_embedding);
            candidate.semantic_source = SemanticSignalSource::Local;
        }

        let Some(selection) = top_candidate_indexes(candidates, limit) else {
            return;
        };

        match self {
            Self::Local => {}
            #[cfg(feature = "openai-embeddings")]
            Self::OpenAi(backend) => {
                let texts = selection
                    .iter()
                    .map(|index| candidates[*index].text.clone())
                    .collect::<Vec<_>>();
                match backend.semantic_similarities(query_text, &texts) {
                    Ok(scores) => {
                        for (index, score) in selection.iter().copied().zip(scores) {
                            candidates[index].semantic = score;
                            candidates[index].semantic_source = SemanticSignalSource::OpenAi;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "OpenAI semantic backend failed; keeping local semantic similarity"
                        );
                        for index in selection {
                            candidates[index].semantic_source = SemanticSignalSource::LocalFallback;
                        }
                    }
                }
            }
            Self::OpenAiUnavailable => {
                for index in selection {
                    candidates[index].semantic_source = SemanticSignalSource::LocalFallback;
                }
            }
        }
    }
}

fn top_candidate_indexes(candidates: &[SemanticCandidate], limit: usize) -> Option<Vec<usize>> {
    if limit == 0 || candidates.is_empty() {
        return None;
    }
    let mut indexes = (0..candidates.len()).collect::<Vec<_>>();
    indexes.sort_by(|left, right| {
        pre_score(&candidates[*right])
            .total_cmp(&pre_score(&candidates[*left]))
            .then_with(|| {
                candidates[*right]
                    .entry
                    .id
                    .0
                    .cmp(&candidates[*left].entry.id.0)
            })
    });
    indexes.truncate(limit.min(candidates.len()));
    Some(indexes)
}

fn pre_score(candidate: &SemanticCandidate) -> f32 {
    0.35 * candidate.signals.overlap.max(0.20)
        + 0.20 * candidate.substring
        + 0.20 * candidate.lexical
        + 0.10 * candidate.alias
        + 0.05 * candidate.signals.recency
        + 0.10 * candidate.signals.trust
}
