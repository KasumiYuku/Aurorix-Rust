mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_gateway::{
    BoxFuture, ConnectRequest, FrameIo, GatewayClient, GatewayEndpoint, GatewayError, GatewaySource,
    GatewayState, StaticTokenSource, Transport,
};
use common::{lock, ready, text, ConnScript, FakeIo, RecordingSink};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(3);

struct UrlTransport {
    connects: AtomicU64,
    scripts: Box<dyn Fn(u64) -> ConnScript + Send + Sync>,
    urls: Mutex<Vec<String>>,
    sockets: Mutex<Vec<Arc<FakeIo>>>,
}

impl UrlTransport {
    fn new(scripts: impl Fn(u64) -> ConnScript + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            connects: AtomicU64::new(0),
            scripts: Box::new(scripts),
            urls: Mutex::new(Vec::new()),
            sockets: Mutex::new(Vec::new()),
        })
    }

    fn connects(&self) -> u64 {
        self.connects.load(Ordering::SeqCst)
    }

    fn urls(&self) -> Vec<String> {
        lock(&self.urls).clone()
    }

    fn socket(&self, index: usize) -> Arc<FakeIo> {
        Arc::clone(&lock(&self.sockets)[index])
    }
}

impl Transport for UrlTransport {
    fn connect<'a>(
        &'a self,
        request: ConnectRequest,
    ) -> BoxFuture<'a, Result<Arc<dyn FrameIo>, GatewayError>> {
        Box::pin(async move {
            let index = self.connects.fetch_add(1, Ordering::SeqCst);
            lock(&self.urls).push(request.url);
            let socket = Arc::new(FakeIo::new((self.scripts)(index)));
            lock(&self.sockets).push(Arc::clone(&socket));
            let io: Arc<dyn FrameIo> = socket;
            Ok(io)
        })
    }
}

struct FakeSource {
    url: String,
    shards: u32,
    delay: Duration,
}

impl GatewaySource for FakeSource {
    fn endpoint(&self) -> BoxFuture<'_, Result<GatewayEndpoint, GatewayError>> {
        let endpoint = GatewayEndpoint::new(self.url.clone(), self.shards);
        Box::pin(async move { Ok(endpoint) })
    }

    fn reauth_delay(&self) -> BoxFuture<'_, Duration> {
        let delay = self.delay;
        Box::pin(async move { delay })
    }
}

fn client(
    url: &str,
    transport: Arc<UrlTransport>,
    source: FakeSource,
    reauth: Duration,
) -> Arc<GatewayClient> {
    let config = common::fast_config(url).with_reauth_delay(reauth);
    Arc::new(
        GatewayClient::with_transport(
            config,
            Arc::new(StaticTokenSource::new("tok-1")),
            RecordingSink::new(),
            transport,
        )
        .with_source(Arc::new(source)),
    )
}

async fn wait_ready(client: &GatewayClient) -> bool {
    common::wait_state(client, GatewayState::Ready, WAIT).await
}

#[tokio::test]
async fn source_supplies_the_url_and_the_shard() {
    let transport = UrlTransport::new(|_| {
        ConnScript::hello(30_000).with_identify(vec![ready("sess-src", "bot-1", "小助手", 1)])
    });
    let client = client(
        "ws://configured.example/",
        Arc::clone(&transport),
        FakeSource {
            url: String::from("wss://official.example"),
            shards: 4,
            delay: Duration::ZERO,
        },
        Duration::ZERO,
    );
    client.start();
    assert!(wait_ready(&client).await, "要能连上来源给的地址");

    assert_eq!(
        transport.urls(),
        vec![String::from("wss://official.example")],
        "连接地址必须来自 /gateway/bot, 不是配置里写死的"
    );
    let sent = transport.socket(0).sent_json();
    assert_eq!(sent[0]["op"], 2);
    assert_eq!(
        sent[0]["d"]["shard"],
        json!([0, 4]),
        "IDENTIFY 要带来源给的分片数"
    );
    assert_eq!(client.shard(), [0, 4]);

    client.shutdown().await;
}

#[tokio::test]
async fn source_delay_overrides_the_configured_reauth_delay() {
    let transport = UrlTransport::new(|index| match index {
        0 => ConnScript::hello(30_000).with_identify(vec![
            ready("sess-old", "bot-1", "小助手", 1),
            text(json!({"op": 9, "d": false})),
        ]),
        _ => ConnScript::hello(30_000)
            .with_identify(vec![ready("sess-new", "bot-1", "小助手", 2)]),
    });
    let client = client(
        "ws://configured.example/",
        Arc::clone(&transport),
        FakeSource {
            url: String::from("wss://official.example"),
            shards: 1,
            delay: Duration::ZERO,
        },
        Duration::from_secs(60),
    );
    client.start();
    assert!(wait_ready(&client).await);
    assert!(
        common::wait_until(|| transport.connects() >= 2, WAIT).await,
        "会话失效后必须按来源给的退避重连, 不能等配置里那 60 秒"
    );
    assert!(
        common::wait_until(|| client.session_id().as_deref() == Some("sess-new"), WAIT).await,
        "重新鉴权后要拿到新会话"
    );

    client.shutdown().await;
}
