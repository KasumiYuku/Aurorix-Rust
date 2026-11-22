use std::sync::Arc;

use tokio::sync::broadcast;

use crate::frame::ProtocolEvent;
use crate::source::EventSink;

#[derive(Debug)]
pub struct BroadcastSink {
    sender: broadcast::Sender<Arc<ProtocolEvent>>,
}

impl BroadcastSink {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<ProtocolEvent>> {
        self.sender.subscribe()
    }

    pub fn receivers(&self) -> usize {
        self.sender.receiver_count()
    }

    pub fn send(&self, event: ProtocolEvent) -> usize {
        self.sender.send(Arc::new(event)).unwrap_or_default()
    }
}

impl EventSink for BroadcastSink {
    fn dispatch(&self, event: ProtocolEvent) {
        let _ = self.sender.send(Arc::new(event));
    }
}
