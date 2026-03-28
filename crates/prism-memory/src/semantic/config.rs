use std::env;

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_MODEL: &str = "text-embedding-3-small";
const DEFAULT_OPENAI_TIMEOUT_SECS: u64 = 20;
const DEFAULT_REMOTE_CANDIDATE_LIMIT: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticBackendKind {
    Local,
    OpenAi,
}

impl Default for SemanticBackendKind {
    fn default() -> Self {
        Self::Local
    }
}

impl SemanticBackendKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "local" | "builtin" | "hashed" => Some(Self::Local),
            "openai" | "openai-embeddings" | "remote" => Some(Self::OpenAi),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiEmbeddingConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout_secs: u64,
    pub candidate_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticMemoryConfig {
    pub preferred_backend: SemanticBackendKind,
    pub openai: Option<OpenAiEmbeddingConfig>,
}

impl Default for SemanticMemoryConfig {
    fn default() -> Self {
        Self {
            preferred_backend: SemanticBackendKind::Local,
            openai: None,
        }
    }
}

impl SemanticMemoryConfig {
    pub fn from_env() -> Self {
        let preferred_backend = env::var("PRISM_SEMANTIC_BACKEND")
            .ok()
            .as_deref()
            .and_then(SemanticBackendKind::parse)
            .unwrap_or_default();
        let openai = openai_config_from_env();
        Self {
            preferred_backend,
            openai,
        }
    }

    pub(crate) fn remote_candidate_limit(&self) -> usize {
        self.openai
            .as_ref()
            .map(|config| config.candidate_limit)
            .unwrap_or(DEFAULT_REMOTE_CANDIDATE_LIMIT)
    }
}

fn openai_config_from_env() -> Option<OpenAiEmbeddingConfig> {
    let api_key = env::var("PRISM_OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("OPENAI_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })?;

    Some(OpenAiEmbeddingConfig {
        api_key,
        base_url: env::var("PRISM_OPENAI_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string()),
        model: env::var("PRISM_OPENAI_EMBEDDING_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string()),
        timeout_secs: env::var("PRISM_OPENAI_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value: &u64| *value > 0)
            .unwrap_or(DEFAULT_OPENAI_TIMEOUT_SECS),
        candidate_limit: env::var("PRISM_OPENAI_EMBEDDING_CANDIDATE_LIMIT")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value: &usize| *value > 0)
            .unwrap_or(DEFAULT_REMOTE_CANDIDATE_LIMIT),
    })
}
