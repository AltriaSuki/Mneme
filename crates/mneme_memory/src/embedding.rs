use anyhow::Result;
use fastembed::{TextEmbedding, InitOptions, EmbeddingModel as FastEmbedModel};
use std::sync::Arc;

pub type Embedding = Vec<f32>;

#[derive(Clone)]
pub struct EmbeddingModel {
    model: Arc<TextEmbedding>,
}

impl EmbeddingModel {
    pub fn new() -> Result<Self> {
        // Initialize with default model (usually BAAI/bge-small-en-v1.5 or similar)
        // We can specify a model that supports Chinese well if needed, 
        // but multilingual-e5-small is a good general choice for mixed usage.
        // For now let's use the default which is often BGE-Small-EN, 
        // but fastembed supports "BAAI/bge-m3" or "intfloat/multilingual-e5-small".
        
        let mut options = InitOptions::default();
        options.model_name = FastEmbedModel::MultilingualE5Small;
        options.show_download_progress = true;

        let model = TextEmbedding::try_new(options)?;

        Ok(Self {
            model: Arc::new(model),
        })
    }

    pub fn embed(&self, text: &str) -> Result<Embedding> {
        let embeddings = self.model.embed(vec![text], None)?;
        // embed returns Vec<Embedding>, we just want the first one
        embeddings.into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Failed to generate embedding"))
    }
    
    pub fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Embedding>> {
        let embeddings = self.model.embed(texts, None)?;
        Ok(embeddings)
    }
}

/// Calculate cosine similarity between two vectors
/// Returns a value between -1.0 and 1.0 (1.0 = identical direction)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}
