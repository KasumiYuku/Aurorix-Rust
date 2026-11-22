use bytes::Bytes;

use crate::error::RenderError;

pub trait AssetLoader: Send + Sync {
    fn load(&self, key: &str) -> Result<Bytes, RenderError>;
}

impl std::fmt::Debug for dyn AssetLoader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AssetLoader")
    }
}

pub struct ChainedAssets {
    loaders: Vec<std::sync::Arc<dyn AssetLoader>>,
}

impl ChainedAssets {
    pub fn new(loaders: Vec<std::sync::Arc<dyn AssetLoader>>) -> Self {
        Self { loaders }
    }
}

impl AssetLoader for ChainedAssets {
    fn load(&self, key: &str) -> Result<Bytes, RenderError> {
        let mut last = None;
        for loader in &self.loaders {
            match loader.load(key) {
                Ok(bytes) => return Ok(bytes),
                Err(error) => last = Some(error),
            }
        }
        match last {
            Some(error) => Err(error),
            None => Err(RenderError::Texture(format!("素材未找到: {key}"))),
        }
    }
}
