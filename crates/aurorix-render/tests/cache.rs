use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{PlaceholderBackend, RenderContext, RenderError, TextureCache};
use bytes::Bytes;

const GUARD: Duration = Duration::from_secs(10);

fn context() -> Arc<RenderContext> {
    RenderContext::new(Arc::new(PlaceholderBackend::new()), 8, 64)
}

#[tokio::test]
async fn texture_hits_and_misses_are_counted() {
    tokio::time::timeout(GUARD, async {
        let cache = TextureCache::new(1024);
        cache
            .insert("alpha", Bytes::from_static(b"alpha"))
            .expect("写入应当成功");
        assert!(cache.get("alpha").is_some(), "命中查询应当返回值");
        assert!(cache.get("alpha").is_some(), "再次命中查询应当返回值");
        assert!(cache.get("beta").is_none(), "未命中查询应当返回空");
        let stats = cache.stats();
        assert_eq!(stats.entries, 1, "缓存条目数应当为 1");
        assert_eq!(stats.bytes, 5, "缓存字节数应当为 5");
        assert_eq!(stats.hits, 2, "命中计数应当为 2");
        assert_eq!(stats.misses, 1, "未命中计数应当为 1");
        assert_eq!(stats.evicted, 0, "不应发生淘汰");
    })
    .await
    .expect("命中统计测试超时");
}

#[tokio::test]
async fn texture_lru_eviction_removes_least_recently_used() {
    tokio::time::timeout(GUARD, async {
        let cache = TextureCache::new(100);
        cache
            .insert("alpha", Bytes::from(vec![1u8; 40]))
            .expect("写入应当成功");
        cache
            .insert("beta", Bytes::from(vec![2u8; 40]))
            .expect("写入应当成功");
        assert!(cache.get("alpha").is_some(), "命中应当刷新访问顺序");
        cache
            .insert("gamma", Bytes::from(vec![3u8; 40]))
            .expect("写入应当成功");

        assert!(cache.get("alpha").is_some(), "最近访问的条目应当保留");
        assert!(cache.get("gamma").is_some(), "新写入的条目应当保留");
        assert!(cache.get("beta").is_none(), "最久未使用的条目应当被淘汰");
        let stats = cache.stats();
        assert_eq!(stats.entries, 2, "淘汰后条目数应当为 2");
        assert_eq!(stats.evicted, 1, "淘汰计数应当为 1");
        assert_eq!(stats.bytes, 80, "淘汰后字节数应当为 80");
    })
    .await
    .expect("LRU 淘汰测试超时");
}

#[tokio::test]
async fn texture_rejects_oversized_entry() {
    tokio::time::timeout(GUARD, async {
        let cache = TextureCache::new(16);
        let outcome = cache.insert("huge", Bytes::from(vec![0u8; 64]));
        match outcome {
            Err(RenderError::Texture(message)) => {
                assert!(message.contains("huge"), "错误消息应当包含纹理键");
            }
            other => panic!("超大纹理应当被拒绝, 实际为 {other:?}"),
        }
        assert_eq!(cache.stats().entries, 0, "被拒绝的纹理不应写入缓存");
    })
    .await
    .expect("超大纹理测试超时");
}

#[tokio::test]
async fn texture_remove_and_invalidate_all() {
    tokio::time::timeout(GUARD, async {
        let cache = TextureCache::new(1024);
        cache
            .insert("alpha", Bytes::from_static(b"alpha"))
            .expect("写入应当成功");
        cache
            .insert("beta", Bytes::from_static(b"beta"))
            .expect("写入应当成功");
        cache.remove("alpha");
        assert!(cache.get("alpha").is_none(), "移除后不应命中");
        assert_eq!(cache.stats().entries, 1, "移除后条目数应当为 1");

        cache.invalidate_all();
        let stats = cache.stats();
        assert_eq!(stats.entries, 0, "清空后条目数应当为 0");
        assert_eq!(stats.bytes, 0, "清空后字节数应当为 0");
        assert!(cache.get("beta").is_none(), "清空后不应命中");
    })
    .await
    .expect("清空缓存测试超时");
}

#[tokio::test]
async fn context_shares_backend_and_textures() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        assert_eq!(
            context.backend().name(),
            "placeholder-cpu",
            "上下文应当携带后端"
        );
        context
            .textures()
            .insert("glyph", Bytes::from_static(b"glyph"))
            .expect("写入应当成功");
        assert_eq!(
            context.textures().stats().entries,
            1,
            "上下文纹理缓存应当可用"
        );
    })
    .await
    .expect("上下文测试超时");
}

#[tokio::test]
async fn concurrent_insert_of_same_key_keeps_single_entry() {
    tokio::time::timeout(GUARD, async {
        let cache = Arc::new(TextureCache::new(4096));
        let writes = Arc::new(AtomicUsize::new(0));
        let joined = tokio::task::spawn_blocking(move || {
            let mut threads = Vec::new();
            for index in 0..8u8 {
                let cache = Arc::clone(&cache);
                let writes = Arc::clone(&writes);
                threads.push(std::thread::spawn(move || {
                    writes.fetch_add(1, Ordering::SeqCst);
                    cache
                        .insert("shared", Bytes::from(vec![index; 16]))
                        .expect("写入应当成功")
                        .len()
                }));
            }
            let mut lengths = Vec::new();
            for thread in threads {
                lengths.push(thread.join().expect("写入线程不应崩溃"));
            }
            (cache, writes.load(Ordering::SeqCst), lengths)
        })
        .await
        .expect("写入线程池不应崩溃");

        let (cache, writes_done, lengths) = joined;
        assert_eq!(writes_done, 8, "八个线程都应当执行写入");
        assert_eq!(lengths.len(), 8, "八个线程都应当返回纹理");
        for length in lengths {
            assert_eq!(length, 16, "每个纹理长度应当一致");
        }
        let stats = cache.stats();
        assert_eq!(stats.entries, 1, "同一键只应当保留一个条目");
        assert_eq!(stats.bytes, 16, "同一键只应当占用一份字节");
        assert!(cache.get("shared").is_some(), "并发写入后应当命中");
    })
    .await
    .expect("并发写入测试超时");
}
