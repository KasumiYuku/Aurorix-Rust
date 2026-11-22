use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use aurorix_log::LogRecord;
use axum::response::sse::Event;
use tokio::sync::broadcast;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::Stream;

pub struct LogStream {
    inner: BroadcastStream<LogRecord>,
}

impl LogStream {
    pub fn new(receiver: broadcast::Receiver<LogRecord>) -> Self {
        Self {
            inner: BroadcastStream::new(receiver),
        }
    }
}

impl Stream for LogStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            match Pin::new(&mut this.inner).poll_next(context) {
                Poll::Ready(Some(Ok(record))) => {
                    let id = record.id.to_string();
                    let payload = serde_json::to_string(&record).unwrap_or_default();
                    let event = Event::default().id(id).data(payload);
                    return Poll::Ready(Some(Ok(event)));
                }
                Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(_)))) => continue,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
