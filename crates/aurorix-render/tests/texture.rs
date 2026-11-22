use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{
    fill_rect, Color, RenderContext, RenderError, RenderRequest, SkiaBackend, TextureCache,
};
use bytes::Bytes;
const GUARD: Duration = Duration::from_secs(60);

fn context() -> Arc<RenderContext> {
    RenderContext::new(Arc::new(SkiaBackend::new()), 8, 8 * 1024 * 1024)
}

fn png_bytes(context: &RenderContext, width: i32, height: i32) -> Bytes {
    context
        .draw(RenderRequest::new(width, height), |canvas, _context| {
            fill_rect(canvas, 0.0, 0.0, 8.0, 8.0, Color::rgb(30, 60, 90));
        })
        .expect("测试用 PNG 应当可以生成")
}

#[tokio::test]
async fn encoded_image_round_trips_through_cache() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let png = png_bytes(&context, 32, 24);
        let cache = TextureCache::new(1024 * 1024);
        let stored = cache
            .insert_encoded("hero", png.clone())
            .expect("编码图像应当可以写入缓存");
        assert_eq!(
            stored.dimensions(),
            Some((32, 24)),
            "缓存内图像尺寸应当正确"
        );
        assert_eq!(stored.len(), png.len(), "缓存条目字节数应当等于编码长度");

        let hit = cache.get("hero").expect("写入后应当命中");
        assert!(hit.image().is_some(), "命中项应当携带解码后的图像");
        assert_eq!(hit.dimensions(), Some((32, 24)), "命中项尺寸应当一致");
        let stats = cache.stats();
        assert_eq!(stats.hits, 1, "命中计数应当为 1");
        assert_eq!(stats.misses, 0, "未命中计数应当为 0");
        assert_eq!(stats.entries, 1, "缓存条目数应当为 1");
        assert_eq!(stats.bytes, png.len() as u64, "缓存字节数应当等于编码长度");
    })
    .await
    .expect("编码图像缓存测试超时");
}

#[tokio::test]
async fn over_capacity_encoded_image_is_evicted() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let first = png_bytes(&context, 16, 16);
        let second = png_bytes(&context, 16, 16);
        let capacity = first.len() as u64 + 8;
        let cache = TextureCache::new(capacity);
        cache
            .insert_encoded("first", first)
            .expect("第一张图应当写入成功");
        cache
            .insert_encoded("second", second)
            .expect("第二张图应当写入成功");
        let stats = cache.stats();
        assert_eq!(stats.evicted, 1, "超出容量应当淘汰一个条目");
        assert_eq!(stats.entries, 1, "淘汰后只应保留一个条目");
        assert!(cache.get("first").is_none(), "最旧的图像应当被淘汰");
        assert!(cache.get("second").is_some(), "最新的图像应当保留");
    })
    .await
    .expect("图像淘汰测试超时");
}

#[tokio::test]
async fn duplicate_key_keeps_single_decoded_copy() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let png = png_bytes(&context, 20, 20);
        let cache = TextureCache::new(1024 * 1024);
        for _ in 0..4 {
            cache
                .insert_encoded("same", png.clone())
                .expect("重复写入应当成功");
        }
        let stats = cache.stats();
        assert_eq!(stats.entries, 1, "同一键只应当保留一个条目");
        assert_eq!(stats.bytes, png.len() as u64, "同一键只应当占用一份字节");
        assert_eq!(stats.evicted, 0, "覆盖同一键不应计入淘汰");
    })
    .await
    .expect("重复键测试超时");
}

#[tokio::test]
async fn invalid_encoded_bytes_are_rejected() {
    tokio::time::timeout(GUARD, async {
        let cache = TextureCache::new(1024 * 1024);
        let outcome = cache.insert_encoded("broken", Bytes::from_static(b"not-a-bitmap"));
        match outcome {
            Err(RenderError::Texture(message)) => {
                assert!(message.contains("broken"), "错误消息应当包含纹理键");
            }
            other => panic!("非法编码应当返回纹理错误, 实际为 {other:?}"),
        }
        assert_eq!(cache.stats().entries, 0, "解码失败不应写入缓存");
        assert_eq!(cache.stats().bytes, 0, "解码失败不应占用字节");
    })
    .await
    .expect("非法编码测试超时");
}

#[tokio::test]
async fn texture_handle_is_shareable_across_threads() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let png = png_bytes(&context, 40, 20);
        let cache = Arc::new(TextureCache::new(1024 * 1024));
        cache.insert_encoded("shared", png).expect("写入应当成功");
        let handle = cache.get("shared").expect("写入后应当命中");
        let workers = (0..8)
            .map(|_| {
                let handle = Arc::clone(&handle);
                std::thread::spawn(move || handle.dimensions())
            })
            .collect::<Vec<_>>();
        for worker in workers {
            let dimensions = worker.join().expect("读取线程不应崩溃");
            assert_eq!(dimensions, Some((40, 20)), "多线程读取的尺寸应当一致");
        }
        assert_eq!(cache.stats().entries, 1, "共享读取不应改变条目数");
    })
    .await
    .expect("纹理共享测试超时");
}
