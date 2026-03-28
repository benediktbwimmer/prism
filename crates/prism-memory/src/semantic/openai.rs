use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::text::cosine_similarity;

use super::config::OpenAiEmbeddingConfig;

const OPENAI_BATCH_SIZE: usize = 16;

pub(super) struct OpenAiEmbeddingBackend {
    client: Client,
    endpoint: String,
    model: String,
    api_key: String,
    cache: Mutex<HashMap<String, Vec<f32>>>,
}

impl OpenAiEmbeddingBackend {
    pub(super) fn new(config: &OpenAiEmbeddingConfig) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(config.timeout_secs))
                .build()
                .context("failed to construct OpenAI embeddings client")?,
            endpoint: embeddings_endpoint(&config.base_url),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn semantic_similarities(&self, query: &str, texts: &[String]) -> Result<Vec<f32>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut needed = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for text in std::iter::once(query).chain(texts.iter().map(String::as_str)) {
            let key = self.cache_key(text);
            if seen.insert(key.clone())
                && !self
                    .cache
                    .lock()
                    .expect("openai embedding cache lock poisoned")
                    .contains_key(&key)
            {
                needed.push(text.to_string());
            }
        }

        for chunk in needed.chunks(OPENAI_BATCH_SIZE) {
            let embeddings = self.request_embeddings(chunk)?;
            let mut cache = self
                .cache
                .lock()
                .expect("openai embedding cache lock poisoned");
            for (text, embedding) in chunk.iter().cloned().zip(embeddings) {
                cache.insert(self.cache_key(&text), embedding);
            }
        }

        let cache = self
            .cache
            .lock()
            .expect("openai embedding cache lock poisoned");
        let query_embedding = cache
            .get(&self.cache_key(query))
            .ok_or_else(|| anyhow!("OpenAI embeddings cache did not contain query vector"))?;
        Ok(texts
            .iter()
            .map(|text| {
                cache.get(&self.cache_key(text)).map_or(0.0, |embedding| {
                    cosine_similarity(query_embedding, embedding)
                })
            })
            .collect())
    }

    fn request_embeddings(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&EmbeddingsRequest {
                model: &self.model,
                input: inputs,
            })
            .send()
            .with_context(|| {
                format!(
                    "failed to call OpenAI embeddings endpoint `{}`",
                    self.endpoint
                )
            })?
            .error_for_status()
            .context("OpenAI embeddings request returned an error status")?;
        let payload: EmbeddingsResponse = response
            .json()
            .context("failed to decode OpenAI embeddings response")?;
        let mut data = payload.data;
        data.sort_by_key(|item| item.index);
        Ok(data.into_iter().map(|item| item.embedding).collect())
    }

    fn cache_key(&self, text: &str) -> String {
        format!("{}::{text}", self.model)
    }
}

fn embeddings_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/embeddings") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/embeddings")
    }
}

#[derive(Serialize)]
struct EmbeddingsRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Deserialize)]
struct EmbeddingDatum {
    index: usize,
    embedding: Vec<f32>,
}
