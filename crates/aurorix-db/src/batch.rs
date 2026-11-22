use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use aurorix_core::Scene;
use sqlx::PgPool;
use tokio::sync::Notify;

use crate::error::{DbError, DbResult};
use crate::inbox::{preview, Message};
use crate::stats::{local_day_key, ConvDelta, DayDelta, StatsBatch, KEEP_DAYS, TRIM_DAILY};

pub const DEFAULT_MAX_BATCH: usize = 256;

pub const DEFAULT_CAPACITY: usize = 4096;

pub const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

pub const TRIM_AFTER_ROWS: usize = 64;

pub const STATS_CONVERSATIONS_UNNEST: &str = "INSERT INTO stats_conversations (scene, id, name, msg_count, friend)\n\t\tSELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::bigint[], $5::boolean[])\n\t\tON CONFLICT (scene, id) DO UPDATE SET\n\t\t\tmsg_count = stats_conversations.msg_count + excluded.msg_count,\n\t\t\tname = CASE WHEN excluded.name <> '' THEN excluded.name ELSE stats_conversations.name END,\n\t\t\tfriend = stats_conversations.friend OR excluded.friend";

pub const STATS_DAILY_UNNEST: &str = "INSERT INTO stats_daily (day, recv, sent, button)\n\t\tSELECT * FROM UNNEST($1::text[], $2::bigint[], $3::bigint[], $4::bigint[])\n\t\tON CONFLICT (day) DO UPDATE SET\n\t\t\trecv = stats_daily.recv + excluded.recv,\n\t\t\tsent = stats_daily.sent + excluded.sent,\n\t\t\tbutton = stats_daily.button + excluded.button";

pub const INBOX_MESSAGES_UNNEST: &str = "INSERT INTO inbox_messages (conv_id, direction, sender_id, sender_name, content, platform_msg_id, created_at)\n\t\tSELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[], $7::bigint[])";

pub const INBOX_CONVERSATIONS_UNNEST: &str = "INSERT INTO inbox_conversations (id, kind, name, last_at, last_preview)\n\t\tSELECT id, kind, '', last_at, last_preview\n\t\tFROM UNNEST($1::text[], $2::text[], $3::bigint[], $4::text[]) AS merged (id, kind, last_at, last_preview)\n\t\tON CONFLICT (id) DO UPDATE SET\n\t\t\tkind = excluded.kind,\n\t\t\tlast_at = GREATEST(inbox_conversations.last_at, excluded.last_at),\n\t\t\tlast_preview = CASE\n\t\t\t\tWHEN excluded.last_at >= inbox_conversations.last_at THEN excluded.last_preview\n\t\t\t\tELSE inbox_conversations.last_preview END";

pub type BatchFuture<'a> = Pin<Box<dyn Future<Output = DbResult<usize>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityPolicy {
    WriteBehind,
    Immediate,
}

impl DurabilityPolicy {
    pub const fn retains_on_failure(self) -> bool {
        matches!(self, Self::WriteBehind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchConfig {
    pub max_batch: usize,
    pub capacity: usize,
    pub flush_interval: Duration,
    pub durability: DurabilityPolicy,
    pub retain_on_failure: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch: DEFAULT_MAX_BATCH,
            capacity: DEFAULT_CAPACITY,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            durability: DurabilityPolicy::WriteBehind,
            retain_on_failure: true,
        }
    }
}

impl BatchConfig {
    pub const fn new() -> Self {
        Self {
            max_batch: DEFAULT_MAX_BATCH,
            capacity: DEFAULT_CAPACITY,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            durability: DurabilityPolicy::WriteBehind,
            retain_on_failure: true,
        }
    }

    pub const fn immediate() -> Self {
        Self {
            max_batch: 1,
            capacity: DEFAULT_CAPACITY,
            flush_interval: Duration::ZERO,
            durability: DurabilityPolicy::Immediate,
            retain_on_failure: false,
        }
    }

    pub const fn with_max_batch(mut self, max_batch: usize) -> Self {
        self.max_batch = if max_batch == 0 { 1 } else { max_batch };
        self
    }

    pub const fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = if capacity == 0 { 1 } else { capacity };
        self
    }

    pub const fn with_flush_interval(mut self, flush_interval: Duration) -> Self {
        self.flush_interval = flush_interval;
        self
    }

    pub const fn with_durability(mut self, durability: DurabilityPolicy) -> Self {
        self.durability = durability;
        self.retain_on_failure = durability.retains_on_failure();
        self
    }

    pub const fn with_retain_on_failure(mut self, retain_on_failure: bool) -> Self {
        self.retain_on_failure = retain_on_failure;
        self
    }
}

pub trait BatchMerge: Clone + Default + Send + 'static {
    fn absorb(&mut self, other: Self);

    fn is_empty(&self) -> bool;

    fn rows(&self) -> usize;
}

pub trait BatchSink<T: BatchMerge>: Send + Sync {
    fn flush_batch(&self, batch: T) -> BatchFuture<'_>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlushReport {
    pub rows: usize,
    pub pending: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatcherStats {
    pub pending: usize,
    pub flushed_batches: u64,
    pub flushed_rows: u64,
    pub failures: u64,
}

pub struct Batcher<T: BatchMerge, S: BatchSink<T>> {
    sink: Arc<S>,
    config: BatchConfig,
    pending: Mutex<T>,
    pending_rows: AtomicUsize,
    flushed_batches: AtomicU64,
    flushed_rows: AtomicU64,
    failures: AtomicU64,
    running: AtomicBool,
    wake: Arc<Notify>,
    background: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl<T: BatchMerge, S: BatchSink<T>> std::fmt::Debug for Batcher<T, S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Batcher")
            .field("max_batch", &self.config.max_batch)
            .field("capacity", &self.config.capacity)
            .field("pending", &self.pending_rows.load(Ordering::Relaxed))
            .finish()
    }
}

impl<T: BatchMerge, S: BatchSink<T>> Batcher<T, S> {
    pub fn new(sink: Arc<S>, config: BatchConfig) -> Self {
        Self {
            sink,
            config,
            pending: Mutex::new(T::default()),
            pending_rows: AtomicUsize::new(0),
            flushed_batches: AtomicU64::new(0),
            flushed_rows: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            running: AtomicBool::new(false),
            wake: Arc::new(Notify::new()),
            background: Mutex::new(None),
        }
    }

    fn lock_pending(&self) -> MutexGuard<'_, T> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_background(&self) -> MutexGuard<'_, Option<tokio::task::JoinHandle<()>>> {
        self.background
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn config(&self) -> BatchConfig {
        self.config
    }

    pub fn sink(&self) -> &Arc<S> {
        &self.sink
    }

    pub fn pending_rows(&self) -> usize {
        self.pending_rows.load(Ordering::Relaxed)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stats(&self) -> BatcherStats {
        BatcherStats {
            pending: self.pending_rows.load(Ordering::Relaxed),
            flushed_batches: self.flushed_batches.load(Ordering::Relaxed),
            flushed_rows: self.flushed_rows.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
        }
    }

    pub fn absorb(&self, delta: T) -> DbResult<()> {
        let mut pending = self.lock_pending();
        let pending_rows = pending.rows();
        let incoming = delta.rows();
        if pending_rows + incoming > self.config.capacity {
            return Err(DbError::BatchBufferFull {
                pending: pending_rows,
                capacity: self.config.capacity,
            });
        }
        pending.absorb(delta);
        self.pending_rows.store(pending.rows(), Ordering::Relaxed);
        Ok(())
    }

    pub async fn record(&self, delta: T) -> DbResult<()> {
        let flush_now = {
            let mut pending = self.lock_pending();
            let pending_rows = pending.rows();
            let incoming = delta.rows();
            if pending_rows + incoming > self.config.capacity {
                return Err(DbError::BatchBufferFull {
                    pending: pending_rows,
                    capacity: self.config.capacity,
                });
            }
            pending.absorb(delta);
            let merged = pending.rows();
            self.pending_rows.store(merged, Ordering::Relaxed);
            self.config.durability == DurabilityPolicy::Immediate || merged >= self.config.max_batch
        };
        if flush_now {
            self.flush().await?;
        }
        Ok(())
    }

    fn take_pending(&self) -> Option<T> {
        let mut pending = self.lock_pending();
        if pending.is_empty() {
            return None;
        }
        let taken = std::mem::take(&mut *pending);
        self.pending_rows.store(0, Ordering::Relaxed);
        Some(taken)
    }

    fn retain(&self, batch: T) {
        let mut pending = self.lock_pending();
        pending.absorb(batch);
        self.pending_rows.store(pending.rows(), Ordering::Relaxed);
    }

    pub async fn flush(&self) -> DbResult<FlushReport> {
        let Some(batch) = self.take_pending() else {
            return Ok(FlushReport {
                rows: 0,
                pending: 0,
            });
        };
        let retained = if self.config.retain_on_failure {
            Some(batch.clone())
        } else {
            None
        };
        match self.sink.flush_batch(batch).await {
            Ok(written) => {
                self.flushed_batches.fetch_add(1, Ordering::Relaxed);
                self.flushed_rows
                    .fetch_add(written as u64, Ordering::Relaxed);
                if written >= TRIM_AFTER_ROWS {
                    aurorix_core::sys::trim_memory();
                }
                Ok(FlushReport {
                    rows: written,
                    pending: self.pending_rows(),
                })
            }
            Err(err) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
                if let Some(retained) = retained {
                    self.retain(retained);
                }
                Err(err)
            }
        }
    }

    pub fn start_background(self: &Arc<Self>, interval: Duration) -> DbResult<()>
    where
        S: 'static,
    {
        let mut slot = self.lock_background();
        if slot.is_some() {
            return Err(DbError::Invalid("写回定时器已经启动".to_string()));
        }
        let tick = if interval.is_zero() {
            if self.config.flush_interval.is_zero() {
                DEFAULT_FLUSH_INTERVAL
            } else {
                self.config.flush_interval
            }
        } else {
            interval
        };
        self.running.store(true, Ordering::SeqCst);
        let batcher = Arc::clone(self);
        let wake = Arc::clone(&self.wake);
        let handle = tokio::spawn(async move {
            loop {
                if !batcher.running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    _ = tokio::time::sleep(tick) => {}
                    _ = wake.notified() => {}
                }
                if !batcher.running.load(Ordering::SeqCst) {
                    break;
                }
                let _ = batcher.flush().await;
            }
        });
        *slot = Some(handle);
        Ok(())
    }

    pub async fn shutdown(&self) -> DbResult<FlushReport> {
        self.running.store(false, Ordering::SeqCst);
        self.wake.notify_one();
        let handle = self.lock_background().take();
        if let Some(handle) = handle {
            handle
                .await
                .map_err(|err| DbError::Invalid(format!("写回定时器异常退出: {err}")))?;
        }
        self.flush().await
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationDelta {
    pub kind: Scene,
    pub last_at: i64,
    pub last_preview: String,
}

impl ConversationDelta {
    fn observe(&mut self, kind: Scene, last_at: i64, last_preview: &str) {
        let newer_preview = last_at >= self.last_at;
        self.kind = kind;
        if newer_preview {
            self.last_preview = last_preview.to_string();
        }
        if last_at > self.last_at {
            self.last_at = last_at;
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxBatch {
    pub messages: Vec<Message>,
    pub conversations: BTreeMap<String, ConversationDelta>,
}

impl InboxBatch {
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.conversations.is_empty()
    }

    pub fn rows(&self) -> usize {
        self.messages.len() + self.conversations.len()
    }

    pub fn note_conversation(
        &mut self,
        conv_id: &str,
        kind: Scene,
        last_at: i64,
        last_preview: &str,
    ) -> &mut Self {
        let entry = self.conversations.entry(conv_id.to_string()).or_default();
        entry.observe(kind, last_at, last_preview);
        self
    }

    pub fn push_message(&mut self, message: Message) -> &mut Self {
        let at = message.created_at.timestamp();
        let short = preview(&message.content);
        self.note_conversation(&message.conv_id, message.kind, at, &short);
        self.messages.push(message);
        self
    }
}

impl BatchMerge for StatsBatch {
    fn absorb(&mut self, other: Self) {
        for ((scene, id), delta) in other.conversations {
            let entry = self.conversations.entry((scene, id)).or_default();
            entry.msg += delta.msg;
            if !delta.name.is_empty() {
                entry.name = delta.name;
            }
            entry.friend |= delta.friend;
        }
        for (day, delta) in other.days {
            let entry = self.days.entry(day).or_default();
            entry.recv += delta.recv;
            entry.sent += delta.sent;
            entry.button += delta.button;
        }
        self.trim |= other.trim;
    }

    fn is_empty(&self) -> bool {
        StatsBatch::is_empty(self)
    }

    fn rows(&self) -> usize {
        self.conversations.len() + self.days.len() + usize::from(self.trim)
    }
}

impl BatchMerge for InboxBatch {
    fn absorb(&mut self, other: Self) {
        for (conv_id, delta) in other.conversations {
            let entry = self.conversations.entry(conv_id).or_default();
            entry.observe(delta.kind, delta.last_at, &delta.last_preview);
        }
        self.messages.extend(other.messages);
    }

    fn is_empty(&self) -> bool {
        InboxBatch::is_empty(self)
    }

    fn rows(&self) -> usize {
        InboxBatch::rows(self)
    }
}

#[derive(Debug, Clone)]
pub struct StatsFlushSink {
    pool: PgPool,
    keep_days: i64,
}

impl StatsFlushSink {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            keep_days: KEEP_DAYS,
        }
    }

    pub fn with_keep_days(pool: PgPool, keep_days: i64) -> Self {
        Self { pool, keep_days }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl BatchSink<StatsBatch> for StatsFlushSink {
    fn flush_batch(&self, batch: StatsBatch) -> BatchFuture<'_> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await?;
            let mut scenes: Vec<String> = Vec::with_capacity(batch.conversations.len());
            let mut ids: Vec<String> = Vec::with_capacity(batch.conversations.len());
            let mut names: Vec<String> = Vec::with_capacity(batch.conversations.len());
            let mut counts: Vec<i64> = Vec::with_capacity(batch.conversations.len());
            let mut friends: Vec<bool> = Vec::with_capacity(batch.conversations.len());
            for ((scene, id), delta) in &batch.conversations {
                scenes.push(scene.as_str().to_string());
                ids.push(id.clone());
                names.push(delta.name.clone());
                counts.push(delta.msg);
                friends.push(delta.friend);
            }
            if !scenes.is_empty() {
                sqlx::query(STATS_CONVERSATIONS_UNNEST)
                    .bind(&scenes)
                    .bind(&ids)
                    .bind(&names)
                    .bind(&counts)
                    .bind(&friends)
                    .execute(&mut *tx)
                    .await?;
            }
            let mut days: Vec<String> = Vec::with_capacity(batch.days.len());
            let mut recv: Vec<i64> = Vec::with_capacity(batch.days.len());
            let mut sent: Vec<i64> = Vec::with_capacity(batch.days.len());
            let mut button: Vec<i64> = Vec::with_capacity(batch.days.len());
            for (day, delta) in &batch.days {
                days.push(day.clone());
                recv.push(delta.recv);
                sent.push(delta.sent);
                button.push(delta.button);
            }
            if !days.is_empty() {
                sqlx::query(STATS_DAILY_UNNEST)
                    .bind(&days)
                    .bind(&recv)
                    .bind(&sent)
                    .bind(&button)
                    .execute(&mut *tx)
                    .await?;
            }
            if batch.trim {
                let cutoff =
                    local_day_key(chrono::Local::now() - chrono::Duration::days(self.keep_days));
                sqlx::query(TRIM_DAILY)
                    .bind(cutoff)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(batch.rows())
        })
    }
}

#[derive(Debug, Clone)]
pub struct InboxFlushSink {
    pool: PgPool,
}

impl InboxFlushSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl BatchSink<InboxBatch> for InboxFlushSink {
    fn flush_batch(&self, batch: InboxBatch) -> BatchFuture<'_> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await?;
            if !batch.messages.is_empty() {
                let mut conv_ids: Vec<String> = Vec::with_capacity(batch.messages.len());
                let mut directions: Vec<String> = Vec::with_capacity(batch.messages.len());
                let mut senders: Vec<String> = Vec::with_capacity(batch.messages.len());
                let mut sender_names: Vec<String> = Vec::with_capacity(batch.messages.len());
                let mut contents: Vec<String> = Vec::with_capacity(batch.messages.len());
                let mut platform_ids: Vec<String> = Vec::with_capacity(batch.messages.len());
                let mut created: Vec<i64> = Vec::with_capacity(batch.messages.len());
                for message in &batch.messages {
                    conv_ids.push(message.conv_id.clone());
                    directions.push(message.direction.as_str().to_string());
                    senders.push(message.sender_id.clone());
                    sender_names.push(message.sender_name.clone());
                    contents.push(message.content.clone());
                    platform_ids.push(message.platform_id.clone());
                    created.push(message.created_at.timestamp());
                }
                sqlx::query(INBOX_MESSAGES_UNNEST)
                    .bind(&conv_ids)
                    .bind(&directions)
                    .bind(&senders)
                    .bind(&sender_names)
                    .bind(&contents)
                    .bind(&platform_ids)
                    .bind(&created)
                    .execute(&mut *tx)
                    .await?;
            }
            if !batch.conversations.is_empty() {
                let mut ids: Vec<String> = Vec::with_capacity(batch.conversations.len());
                let mut kinds: Vec<String> = Vec::with_capacity(batch.conversations.len());
                let mut last_at: Vec<i64> = Vec::with_capacity(batch.conversations.len());
                let mut previews: Vec<String> = Vec::with_capacity(batch.conversations.len());
                for (conv_id, delta) in &batch.conversations {
                    ids.push(conv_id.clone());
                    kinds.push(delta.kind.as_str().to_string());
                    last_at.push(delta.last_at);
                    previews.push(delta.last_preview.clone());
                }
                sqlx::query(INBOX_CONVERSATIONS_UNNEST)
                    .bind(&ids)
                    .bind(&kinds)
                    .bind(&last_at)
                    .bind(&previews)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(batch.rows())
        })
    }
}

pub type StatsBatcher = Batcher<StatsBatch, StatsFlushSink>;

pub type InboxBatcher = Batcher<InboxBatch, InboxFlushSink>;

#[derive(Debug, Clone)]
pub struct WriteBehind {
    pub stats: Arc<StatsBatcher>,
    pub inbox: Arc<InboxBatcher>,
}

impl WriteBehind {
    pub fn start_background(&self, interval: Duration) -> DbResult<()> {
        self.stats.start_background(interval)?;
        self.inbox.start_background(interval)
    }

    pub async fn flush(&self) -> DbResult<(FlushReport, FlushReport)> {
        let stats = self.stats.flush().await?;
        let inbox = self.inbox.flush().await?;
        Ok((stats, inbox))
    }

    pub async fn shutdown(&self) -> DbResult<()> {
        self.stats.shutdown().await?;
        self.inbox.shutdown().await?;
        Ok(())
    }

    pub fn stats_counters(&self) -> BatcherStats {
        self.stats.stats()
    }

    pub fn inbox_counters(&self) -> BatcherStats {
        self.inbox.stats()
    }
}

pub fn stats_rows_of(batch: &StatsBatch) -> usize {
    batch.rows()
}

pub fn conv_delta(name: &str, msg: i64, friend: bool) -> ConvDelta {
    ConvDelta {
        name: name.to_string(),
        msg,
        friend,
    }
}

pub fn day_delta(recv: i64, sent: i64, button: i64) -> DayDelta {
    DayDelta { recv, sent, button }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[derive(Default)]
    struct RecordingSink {
        batches: Mutex<Vec<StatsBatch>>,
        inbox: Mutex<Vec<InboxBatch>>,
        fail_times: AtomicUsize,
    }

    impl RecordingSink {
        fn batches(&self) -> Vec<StatsBatch> {
            self.batches
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }

        fn inbox_batches(&self) -> Vec<InboxBatch> {
            self.inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl BatchSink<StatsBatch> for RecordingSink {
        fn flush_batch(&self, batch: StatsBatch) -> BatchFuture<'_> {
            Box::pin(async move {
                if self.fail_times.load(Ordering::SeqCst) > 0 {
                    self.fail_times.fetch_sub(1, Ordering::SeqCst);
                    return Err(DbError::Invalid("模拟写失败".to_string()));
                }
                let rows = batch.rows();
                self.batches
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(batch);
                Ok(rows)
            })
        }
    }

    impl BatchSink<InboxBatch> for RecordingSink {
        fn flush_batch(&self, batch: InboxBatch) -> BatchFuture<'_> {
            Box::pin(async move {
                let rows = batch.rows();
                self.inbox
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(batch);
                Ok(rows)
            })
        }
    }

    fn conv(name: &str, msg: i64, friend: bool) -> StatsBatch {
        let mut batch = StatsBatch::default();
        batch.conversations.insert(
            (Scene::Group, "g1".to_string()),
            ConvDelta {
                name: name.to_string(),
                msg,
                friend,
            },
        );
        batch
    }

    fn day(key: &str, recv: i64, sent: i64, button: i64) -> StatsBatch {
        let mut batch = StatsBatch::default();
        batch
            .days
            .insert(key.to_string(), DayDelta { recv, sent, button });
        batch
    }

    #[tokio::test]
    async fn two_deltas_on_the_same_conversation_and_day_collapse_into_one_row() {
        let sink = Arc::new(RecordingSink::default());
        let batcher = Batcher::new(Arc::clone(&sink), BatchConfig::new().with_max_batch(64));
        batcher.record(conv("群一", 1, false)).await.expect("记录");
        batcher.record(conv("群二", 2, true)).await.expect("记录");
        batcher
            .record(day("2024-01-01", 1, 2, 3))
            .await
            .expect("记录");
        batcher
            .record(day("2024-01-01", 4, 0, 1))
            .await
            .expect("记录");
        assert_eq!(batcher.pending_rows(), 2, "一个会话加一天只应当占两行");
        let report = batcher.flush().await.expect("刷新");
        assert_eq!(report.rows, 2);
        let batches = sink.batches();
        assert_eq!(batches.len(), 1, "同一批只应当写一次");
        let merged = &batches[0];
        let entry = &merged.conversations[&(Scene::Group, "g1".to_string())];
        assert_eq!(entry.msg, 3, "消息数应当求和");
        assert_eq!(entry.name, "群二", "名字应当是最后一次写入");
        assert!(entry.friend, "私聊标记应当是或");
        let daily = &merged.days["2024-01-01"];
        assert_eq!(daily.recv, 5);
        assert_eq!(daily.sent, 2);
        assert_eq!(daily.button, 4);
        assert_eq!(batcher.pending_rows(), 0, "刷新后缓冲区应当清空");
        assert_eq!(batcher.stats().flushed_batches, 1);
        assert_eq!(batcher.stats().flushed_rows, 2);
    }

    #[tokio::test]
    async fn reaching_max_batch_flushes_immediately() {
        let sink = Arc::new(RecordingSink::default());
        let batcher = Batcher::new(Arc::clone(&sink), BatchConfig::new().with_max_batch(2));
        batcher.record(conv("群一", 1, false)).await.expect("记录");
        assert_eq!(sink.batches().len(), 0, "未到阈值不该写");
        batcher
            .record(day("2024-01-02", 1, 0, 0))
            .await
            .expect("记录");
        assert_eq!(sink.batches().len(), 1, "到达阈值应当立刻写");
        assert_eq!(batcher.pending_rows(), 0);
    }

    #[tokio::test]
    async fn immediate_durability_writes_on_every_record() {
        let sink = Arc::new(RecordingSink::default());
        let batcher = Batcher::new(Arc::clone(&sink), BatchConfig::immediate());
        batcher.record(conv("群一", 1, false)).await.expect("记录");
        batcher.record(conv("群二", 1, false)).await.expect("记录");
        assert_eq!(sink.batches().len(), 2);
        assert_eq!(batcher.pending_rows(), 0);
    }

    #[tokio::test]
    async fn capacity_overflow_reports_backpressure_and_keeps_pending_rows() {
        let sink = Arc::new(RecordingSink::default());
        let batcher = Batcher::new(
            Arc::clone(&sink),
            BatchConfig::new().with_capacity(2).with_max_batch(64),
        );
        batcher.record(conv("群一", 1, false)).await.expect("记录");
        batcher
            .record(day("2024-01-03", 1, 0, 0))
            .await
            .expect("记录");
        let mut third = StatsBatch::default();
        third.conversations.insert(
            (Scene::Group, "g2".to_string()),
            ConvDelta {
                name: String::new(),
                msg: 1,
                friend: false,
            },
        );
        let err = batcher.record(third).await.unwrap_err();
        match err {
            DbError::BatchBufferFull { pending, capacity } => {
                assert_eq!(pending, 2);
                assert_eq!(capacity, 2);
            }
            other => panic!("期望缓冲区背压错误, 实际 {other}"),
        }
        assert_eq!(batcher.pending_rows(), 2, "背压不该丢掉已有数据");
        assert!(!batcher.stats().pending.to_string().is_empty());
    }

    #[tokio::test]
    async fn write_behind_failure_keeps_rows_pending() {
        let sink = Arc::new(RecordingSink::default());
        sink.fail_times.store(1, Ordering::SeqCst);
        let batcher = Batcher::new(Arc::clone(&sink), BatchConfig::new().with_max_batch(64));
        batcher.record(conv("群一", 1, false)).await.expect("记录");
        let err = batcher.flush().await.unwrap_err();
        assert!(matches!(err, DbError::Invalid(_)));
        assert_eq!(batcher.pending_rows(), 1, "写失败必须保留待写数据");
        assert_eq!(batcher.stats().failures, 1);
        batcher.flush().await.expect("重试");
        assert_eq!(batcher.pending_rows(), 0);
        assert_eq!(sink.batches().len(), 1);
    }

    #[tokio::test]
    async fn immediate_failure_reports_to_the_caller_without_retaining() {
        let sink = Arc::new(RecordingSink::default());
        sink.fail_times.store(1, Ordering::SeqCst);
        let batcher = Batcher::new(Arc::clone(&sink), BatchConfig::immediate());
        let err = batcher.record(conv("群一", 1, false)).await.unwrap_err();
        assert!(matches!(err, DbError::Invalid(_)));
        assert_eq!(batcher.pending_rows(), 0, "立即模式下由调用方重试");
    }

    #[tokio::test]
    async fn empty_flush_does_not_touch_the_sink() {
        let sink = Arc::new(RecordingSink::default());
        let batcher: Batcher<StatsBatch, RecordingSink> =
            Batcher::new(Arc::clone(&sink), BatchConfig::new());
        let report = batcher.flush().await.expect("空刷新");
        assert_eq!(report.rows, 0);
        assert_eq!(batcher.stats().flushed_batches, 0);
        assert_eq!(sink.batches().len(), 0);
    }

    #[tokio::test]
    async fn shutdown_flushes_remaining_rows_and_stops_the_timer() {
        let sink = Arc::new(RecordingSink::default());
        let batcher = Arc::new(Batcher::new(
            Arc::clone(&sink),
            BatchConfig::new().with_max_batch(64),
        ));
        batcher
            .start_background(Duration::from_secs(3600))
            .expect("启动");
        assert!(batcher.is_running());
        batcher.record(conv("群一", 1, false)).await.expect("记录");
        batcher.record(conv("群二", 2, true)).await.expect("记录");
        batcher
            .record(day("2024-01-04", 3, 0, 0))
            .await
            .expect("记录");
        assert_eq!(sink.batches().len(), 0);
        let report = batcher.shutdown().await.expect("关闭");
        assert!(report.rows > 0, "关闭必须把剩余数据写完");
        assert!(!batcher.is_running());
        let written: usize = sink
            .batches()
            .iter()
            .map(|batch| batch.rows())
            .sum::<usize>();
        assert_eq!(written, report.rows, "关闭后不该有丢行");
        assert_eq!(batcher.pending_rows(), 0);
    }

    #[tokio::test]
    async fn background_timer_flushes_on_interval() {
        let sink = Arc::new(RecordingSink::default());
        let batcher = Arc::new(Batcher::new(
            Arc::clone(&sink),
            BatchConfig::new().with_max_batch(64),
        ));
        batcher
            .start_background(Duration::from_millis(10))
            .expect("启动");
        batcher.record(conv("群一", 1, false)).await.expect("记录");
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(sink.batches().len(), 1, "定时器应当把数据刷出去");
        assert_eq!(batcher.pending_rows(), 0);
        batcher.shutdown().await.expect("关闭");
    }

    #[tokio::test]
    async fn starting_the_timer_twice_is_rejected() {
        let sink = Arc::new(RecordingSink::default());
        let batcher: Arc<Batcher<StatsBatch, RecordingSink>> =
            Arc::new(Batcher::new(Arc::clone(&sink), BatchConfig::new()));
        batcher
            .start_background(Duration::from_secs(60))
            .expect("启动");
        let err = batcher
            .start_background(Duration::from_secs(60))
            .expect_err("重复启动");
        assert!(matches!(err, DbError::Invalid(_)));
        batcher.shutdown().await.expect("关闭");
    }

    #[tokio::test]
    async fn inbox_batch_merges_conversations_with_last_writer_wins_preview() {
        let sink = Arc::new(RecordingSink::default());
        let batcher = Batcher::new(Arc::clone(&sink), BatchConfig::new().with_max_batch(64));
        let mut batch = InboxBatch::default();
        batch.note_conversation("c1", Scene::Group, 100, "旧");
        batch.note_conversation("c1", Scene::Channel, 100, "新");
        batch.note_conversation("c1", Scene::Channel, 90, "更旧");
        assert_eq!(batch.conversations["c1"].last_at, 100, "时间取最大");
        assert_eq!(
            batch.conversations["c1"].last_preview, "新",
            "同一时间戳最后一次写入胜出"
        );
        assert_eq!(batch.conversations["c1"].kind, Scene::Channel);
        batcher.record(batch).await.expect("记录");
        let report = batcher.flush().await.expect("刷新");
        assert_eq!(report.rows, 1);
        let written = sink.inbox_batches();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].conversations.len(), 1);
    }

    #[test]
    fn unnest_sql_keeps_go_merge_semantics() {
        assert!(STATS_CONVERSATIONS_UNNEST.contains("UNNEST($1::text[], $2::text[]"));
        assert!(STATS_CONVERSATIONS_UNNEST
            .contains("msg_count = stats_conversations.msg_count + excluded.msg_count"));
        assert!(
            STATS_CONVERSATIONS_UNNEST.contains("CASE WHEN excluded.name <> '' THEN excluded.name")
        );
        assert!(STATS_CONVERSATIONS_UNNEST
            .contains("friend = stats_conversations.friend OR excluded.friend"));
        assert!(STATS_DAILY_UNNEST.contains("recv = stats_daily.recv + excluded.recv"));
        assert!(INBOX_CONVERSATIONS_UNNEST
            .contains("GREATEST(inbox_conversations.last_at, excluded.last_at)"));
        assert!(INBOX_CONVERSATIONS_UNNEST
            .contains("WHEN excluded.last_at >= inbox_conversations.last_at"));
        assert!(INBOX_MESSAGES_UNNEST.contains("$7::bigint[]"));
    }
}
