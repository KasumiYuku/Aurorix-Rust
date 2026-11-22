use std::collections::VecDeque;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::response::sse::Event;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::Stream;

use crate::state::AdminState;

pub const TOPIC_OVERVIEW: &str = "overview.snapshot";
pub const TOPIC_JOBS: &str = "jobs.list";
pub const TOPIC_LOG: &str = "log.entry";
pub const TOPIC_INBOX: &str = "inbox.message";

pub const LIVE_BUFFER: usize = 256;

pub const OVERVIEW_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub topic: String,
    pub seq: i64,
    pub at: i64,
    pub data: Value,
}

#[derive(Debug)]
pub struct LiveBus {
    tx: broadcast::Sender<Envelope>,
    seq: AtomicI64,
    replay: Mutex<VecDeque<Envelope>>,
}

impl Default for LiveBus {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(LIVE_BUFFER);
        Self {
            tx,
            seq: AtomicI64::new(0),
            replay: Mutex::new(VecDeque::with_capacity(LIVE_BUFFER)),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn subscribers(&self) -> usize {
        self.tx.receiver_count()
    }

    pub fn publish(&self, topic: &str, data: Value) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let envelope = Envelope {
            topic: topic.to_string(),
            seq,
            at: now_ms(),
            data,
        };
        {
            let mut replay = self.replay_lock();
            replay.push_back(envelope.clone());
            while replay.len() > LIVE_BUFFER {
                replay.pop_front();
            }
        }
        let _ = self.tx.send(envelope);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }

    pub fn replay_after(&self, last_event_id: i64) -> Vec<Envelope> {
        if last_event_id <= 0 {
            return Vec::new();
        }
        self.replay_lock()
            .iter()
            .filter(|envelope| envelope.seq > last_event_id)
            .cloned()
            .collect()
    }

    pub fn stream(self: &Arc<Self>, last_event_id: i64) -> LiveStream {
        let receiver = self.subscribe();
        let pending: VecDeque<Envelope> = self.replay_after(last_event_id).into();
        LiveStream {
            bus: Arc::clone(self),
            inner: BroadcastStream::new(receiver),
            pending,
            last_seq: last_event_id,
        }
    }

    fn replay_lock(&self) -> MutexGuard<'_, VecDeque<Envelope>> {
        self.replay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn frame(envelope: &Envelope) -> Event {
    let payload = serde_json::to_string(envelope).unwrap_or_default();
    Event::default().id(envelope.seq.to_string()).data(payload)
}

pub struct LiveStream {
    bus: Arc<LiveBus>,
    inner: BroadcastStream<Envelope>,
    pending: VecDeque<Envelope>,
    last_seq: i64,
}

impl LiveStream {
    pub fn last_seq(&self) -> i64 {
        self.last_seq
    }
}

impl Stream for LiveStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(envelope) = this.pending.pop_front() {
                if envelope.seq <= this.last_seq {
                    continue;
                }
                this.last_seq = envelope.seq;
                return Poll::Ready(Some(Ok(frame(&envelope))));
            }
            match Pin::new(&mut this.inner).poll_next(context) {
                Poll::Ready(Some(Ok(envelope))) => {
                    if envelope.seq <= this.last_seq {
                        continue;
                    }
                    this.last_seq = envelope.seq;
                    return Poll::Ready(Some(Ok(frame(&envelope))));
                }
                Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(_)))) => {
                    let missed = this.bus.replay_after(this.last_seq);
                    this.pending.extend(missed);
                    continue;
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

pub fn spawn_live_tasks(state: AdminState, bus: Arc<LiveBus>) -> tokio::task::JoinHandle<()> {
    let log_bus = Arc::clone(&bus);
    let mut logs = state.logs.subscribe();
    tokio::spawn(async move {
        loop {
            match logs.recv().await {
                Ok(record) => {
                    if log_bus.subscribers() == 0 {
                        continue;
                    }
                    if let Ok(value) = serde_json::to_value(&record) {
                        log_bus.publish(TOPIC_LOG, value);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(OVERVIEW_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if bus.subscribers() == 0 {
                continue;
            }
            let overview = crate::routes::overview_snapshot(&state).await;
            bus.publish(TOPIC_OVERVIEW, overview);
            let jobs = serde_json::to_value(state.jobs.jobs()).unwrap_or(Value::Null);
            bus.publish(TOPIC_JOBS, jobs);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn replays_events_after_last_event_id() {
        let bus = LiveBus::shared();
        bus.publish("t", json!({ "n": 1 }));
        bus.publish("t", json!({ "n": 2 }));
        bus.publish("t", json!({ "n": 3 }));
        let mut stream = bus.stream(1);
        let replayed = stream.next().await.expect("必须回放断线期间的事件");
        assert!(replayed.is_ok(), "回放事件不该带错误");
        assert_eq!(stream.last_seq(), 2, "回放之后游标必须前进");
        let second = stream.next().await.expect("第二条回放事件");
        assert!(second.is_ok());
        assert_eq!(stream.last_seq(), 3);
    }

    #[tokio::test]
    async fn replay_without_cursor_starts_from_live_events() {
        let bus = LiveBus::shared();
        bus.publish("t", json!({ "n": 1 }));
        assert!(bus.replay_after(0).is_empty(), "没有游标时不回放历史");
        assert_eq!(bus.replay_after(1).len(), 0);
        assert_eq!(bus.replay_after(-1).len(), 0);
    }

    #[tokio::test]
    async fn replay_buffer_is_bounded() {
        let bus = LiveBus::shared();
        for index in 0..(LIVE_BUFFER + 32) {
            bus.publish("t", json!({ "n": index }));
        }
        let replayed = bus.replay_after(1);
        assert_eq!(replayed.len(), LIVE_BUFFER, "环形缓冲不能无限增长");
        assert_eq!(replayed[0].seq, 33);
    }

    #[tokio::test]
    async fn lagged_subscriber_is_backfilled_from_the_ring() {
        let bus = LiveBus::shared();
        bus.publish("t", json!({ "n": 0 }));
        let mut stream = bus.stream(1);
        for index in 1..(LIVE_BUFFER + 16) {
            bus.publish("t", json!({ "n": index }));
        }
        let first = stream.next().await.expect("落后时也必须拿到事件");
        assert!(first.is_ok());
        assert!(stream.last_seq() > 1, "补发之后游标必须前进");
    }
}
