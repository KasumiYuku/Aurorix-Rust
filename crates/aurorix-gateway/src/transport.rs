use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::GatewayError;
use crate::source::BoxFuture;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Text(String),
    Binary(Vec<u8>),
    Close(u16),
}

impl Frame {
    pub fn close_code(&self) -> Option<u16> {
        match self {
            Frame::Close(code) => Some(*code),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Frame::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }

    pub fn text_of(&self) -> Option<String> {
        match self {
            Frame::Text(text) => Some(text.clone()),
            Frame::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
            Frame::Close(_) => None,
        }
    }
}

pub trait FrameIo: Send + Sync {
    fn recv(&self) -> BoxFuture<'_, Result<Frame, GatewayError>>;
    fn send(&self, frame: Frame) -> BoxFuture<'_, Result<(), GatewayError>>;
    fn close(&self, code: u16) -> BoxFuture<'_, Result<(), GatewayError>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl ConnectRequest {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
        }
    }

    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }
}

pub trait Transport: Send + Sync {
    fn connect<'a>(
        &'a self,
        request: ConnectRequest,
    ) -> BoxFuture<'a, Result<Arc<dyn FrameIo>, GatewayError>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WebSocketTransport;

impl WebSocketTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Transport for WebSocketTransport {
    fn connect<'a>(
        &'a self,
        request: ConnectRequest,
    ) -> BoxFuture<'a, Result<Arc<dyn FrameIo>, GatewayError>> {
        Box::pin(async move {
            let io = open(request).await?;
            let shared: Arc<dyn FrameIo> = Arc::new(io);
            Ok(shared)
        })
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct WsIo {
    writer: Mutex<SplitSink<Socket, Message>>,
    reader: Mutex<SplitStream<Socket>>,
}

impl WsIo {
    fn new(socket: Socket) -> Self {
        let (writer, reader) = socket.split();
        Self {
            writer: Mutex::new(writer),
            reader: Mutex::new(reader),
        }
    }

    fn decode(message: Message) -> Option<Frame> {
        match message {
            Message::Text(text) => Some(Frame::Text(text.as_str().to_string())),
            Message::Binary(bytes) => Some(Frame::Binary(bytes.to_vec())),
            Message::Close(frame) => Some(Frame::Close(match frame {
                Some(frame) => u16::from(frame.code),
                None => 1005,
            })),
            _ => None,
        }
    }

    fn encode(frame: Frame) -> Message {
        match frame {
            Frame::Text(text) => Message::Text(text.into()),
            Frame::Binary(bytes) => Message::Binary(bytes.into()),
            Frame::Close(code) => Message::Close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: "".into(),
            })),
        }
    }

    fn map_send_error(err: tungstenite::Error) -> GatewayError {
        match err {
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                GatewayError::Transport("连接已关闭, 无法发送".to_string())
            }
            other => GatewayError::Transport(other.to_string()),
        }
    }
}

impl FrameIo for WsIo {
    fn recv(&self) -> BoxFuture<'_, Result<Frame, GatewayError>> {
        Box::pin(async move {
            let mut reader = self.reader.lock().await;
            loop {
                match reader.next().await {
                    Some(Ok(message)) => {
                        if let Some(frame) = WsIo::decode(message) {
                            return Ok(frame);
                        }
                    }
                    Some(Err(tungstenite::Error::ConnectionClosed))
                    | Some(Err(tungstenite::Error::AlreadyClosed))
                    | None => return Ok(Frame::Close(1006)),
                    Some(Err(err)) => return Err(GatewayError::Transport(err.to_string())),
                }
            }
        })
    }

    fn send(&self, frame: Frame) -> BoxFuture<'_, Result<(), GatewayError>> {
        Box::pin(async move {
            let message = WsIo::encode(frame);
            let mut writer = self.writer.lock().await;
            writer.send(message).await.map_err(WsIo::map_send_error)
        })
    }

    fn close(&self, code: u16) -> BoxFuture<'_, Result<(), GatewayError>> {
        Box::pin(async move {
            let mut writer = self.writer.lock().await;
            let frame = CloseFrame {
                code: CloseCode::from(code),
                reason: "".into(),
            };
            let _ = writer.send(Message::Close(Some(frame))).await;
            let _ = writer.close().await;
            Ok(())
        })
    }
}

fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

async fn open(request: ConnectRequest) -> Result<WsIo, GatewayError> {
    install_crypto_provider();
    let mut upstream = request
        .url
        .as_str()
        .into_client_request()
        .map_err(|err| GatewayError::Handshake(err.to_string()))?;
    for (name, value) in request.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| GatewayError::Handshake(err.to_string()))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|err| GatewayError::Handshake(err.to_string()))?;
        upstream.headers_mut().insert(name, value);
    }
    let (socket, _response) =
        tokio_tungstenite::connect_async_tls_with_config(upstream, None, false, None)
            .await
            .map_err(|err| GatewayError::Handshake(err.to_string()))?;
    Ok(WsIo::new(socket))
}
