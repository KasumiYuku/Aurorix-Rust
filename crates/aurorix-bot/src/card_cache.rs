use std::future::Future;

use aurorix_render::RenderError;
use bytes::Bytes;

use crate::ports::Rendered;

pub use aurorix_render::card_cache::{budget_bytes, clear, entries, hits, misses, renders, used_bytes};

pub async fn cached<F>(key: u64, filename: String, render: F) -> Result<Rendered, RenderError>
where
    F: Future<Output = Result<Vec<u8>, RenderError>>,
{
    let bytes: Bytes = aurorix_render::card_cache::cached(key, render).await?;
    Ok(Rendered::Image {
        bytes: bytes.to_vec(),
        filename,
    })
}
