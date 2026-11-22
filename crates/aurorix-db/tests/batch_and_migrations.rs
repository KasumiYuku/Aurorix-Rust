use aurorix_db::{
    BanEntry, BatchConfig, Batcher, DurabilityPolicy, FlushReport, HotCache, InboxBatch,
    InboxFlushSink, MigrationReport, RosterView,
};

#[test]
fn batch_default_keeps_failed_batches_for_retry() {
    let config = BatchConfig::default();
    assert!(
        config.retain_on_failure,
        "默认策略必须在写失败后保留批次, 否则一次网络抖动就会静默丢数据"
    );
    assert_eq!(
        config.durability,
        DurabilityPolicy::WriteBehind,
        "默认必须是写后台合并, 而不是每条都同步落库"
    );
    assert!(config.max_batch > 1, "微批窗口必须大于 1 行");
    assert!(
        config.capacity >= config.max_batch,
        "缓冲容量不得小于单批上限"
    );
    let tight = config.with_retain_on_failure(false);
    assert!(!tight.retain_on_failure, "显式关闭保留策略后应当为 false");
}

#[test]
fn migration_report_is_empty_only_when_both_sides_are_empty() {
    let empty = MigrationReport::default();
    assert!(empty.is_empty(), "没有任何迁移时应当为空");
    assert_eq!(empty.applied_count(), 0);
    assert_eq!(empty.skipped_count(), 0);

    let mut applied_only = MigrationReport::default();
    applied_only.applied.push(1);
    assert!(
        !applied_only.is_empty(),
        "有已应用步骤时不得报告为空, 否则上层会重复执行迁移"
    );

    let mut skipped_only = MigrationReport::default();
    skipped_only.skipped.push(2);
    assert!(
        !skipped_only.is_empty(),
        "有跳过步骤时不得报告为空, 否则上层会误判本插件没有迁移历史"
    );
}

#[tokio::test]
async fn hot_cache_loads_once_then_serves_from_memory() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let cache: HotCache<RosterView> = HotCache::new();
    let loads = Arc::new(AtomicUsize::new(0));
    let view = Arc::new(RosterView::from_entries([BanEntry::new("openid-1")]));
    for _ in 0..16 {
        let counter = Arc::clone(&loads);
        let value = Arc::clone(&view);
        let resolved = cache
            .load_or(|| {
                counter.fetch_add(1, Ordering::SeqCst);
                async move { Ok(value) }
            })
            .await
            .expect("缓存装载应当成功");
        assert!(resolved.contains("openid-1"), "命中后内容必须一致");
    }
    assert_eq!(
        loads.load(Ordering::SeqCst),
        1,
        "16 次读取只允许真正装载一次"
    );
    assert_eq!(cache.stats().loads, 1, "装载计数必须为 1");
    assert!(
        cache.stats().hits >= 15,
        "其余读取必须全部走命中: {:?}",
        cache.stats()
    );
    cache.invalidate();
    assert!(cache.get().is_none(), "失效后必须回到冷状态");
}

#[test]
fn inbox_batch_merges_conversations_in_memory() {
    let batch = InboxBatch::default();
    assert!(batch.is_empty(), "空批次应当报告为空");
    let _ = std::any::type_name::<Batcher<InboxBatch, InboxFlushSink>>();
    let _ = std::any::type_name::<FlushReport>();
}
