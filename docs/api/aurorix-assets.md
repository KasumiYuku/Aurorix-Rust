# aurorix-assets API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/breaker.rs`

- `pub const DEFAULT_FAIL_THRESHOLD: u32 = 3`
- `pub const DEFAULT_COOLDOWN_MIN: Duration = Duration::from_secs(60)`
- `pub const DEFAULT_COOLDOWN_MAX: Duration = Duration::from_secs(120)`
- `pub trait BreakerClock: Send + Sync`
- `pub struct SystemClock`
- `pub struct BreakerConfig`
- `impl BreakerConfig`
  - `pub fn new(fail_threshold: u32, cooldown_min: Duration, cooldown_max: Duration) -> Self`
  - `pub fn with_fail_threshold(mut self, fail_threshold: u32) -> Self`
  - `pub fn with_cooldowns(mut self, cooldown_min: Duration, cooldown_max: Duration) -> Self`
  - `pub fn cooldown_for(&self, open_count: u64) -> Duration`
- `pub enum BreakerVerdict`
- `impl BreakerVerdict`
  - `pub fn blocked(self) -> bool`
- `pub enum BreakerEvent`
- `impl BreakerEvent`
  - `pub fn opened(self) -> bool`
- `pub struct BreakerSnapshot`
- `impl Entry`
- `pub struct CircuitBreaker`
- `impl CircuitBreaker`
  - `pub fn new(config: BreakerConfig) -> Self`
  - `pub fn with_clock(config: BreakerConfig, clock: Arc<dyn BreakerClock>) -> Self`
  - `pub fn config(&self) -> BreakerConfig`
  - `pub fn verdict(&self, provider: &str) -> BreakerVerdict`
  - `pub fn is_open(&self, provider: &str) -> bool`
  - `pub fn record_success(&self, provider: &str)`
  - `pub fn record_failure(&self, provider: &str) -> BreakerEvent`
  - `pub fn reset(&self, provider: &str)`
  - `pub fn reset_all(&self)`
  - `pub fn fail_streak(&self, provider: &str) -> u32`
  - `pub fn snapshots(&self) -> Vec<BreakerSnapshot>`
  - `pub fn open_providers(&self) -> Vec<String>`

## `src/cache.rs`

- `pub const DEFAULT_CAPACITY: usize = 512`
- `pub struct DedupEntry`
- `pub struct UploadDedupCache`
- `impl UploadDedupCache`
  - `pub fn new(capacity: usize) -> Self`
  - `pub fn capacity(&self) -> usize`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn get(&self, key: &str) -> Option<DedupEntry>`
  - `pub fn put(&self, key: impl Into<String>, url: impl Into<String>, provider: impl Into<String>)`
  - `pub fn clear(&self)`

## `src/config.rs`

- `pub struct HostConfig`
- `pub struct ProviderItem`
- `impl ProviderItem`
  - `pub fn new(name: impl Into<String>) -> Self`
  - `pub fn with_priority(mut self, priority: i32) -> Self`
  - `pub fn with_enabled(mut self, enabled: bool) -> Self`
  - `pub fn with_probe(mut self, probe: bool) -> Self`
  - `pub fn with_config(mut self, key: &str, value: impl Into<Value>) -> Self`
  - `pub fn is_enabled(&self) -> bool`

## `src/error.rs`

- `pub enum AssetError`
- `impl AssetError`
  - `pub fn message(message: impl Into<String>) -> Self`
  - `pub fn status(status: u16, body: impl Into<String>) -> Self`
  - `pub fn transport(message: impl Into<String>) -> Self`
- `pub type AssetResult<T> = Result<T, AssetError>`

## `src/host.rs`

- `pub const MAX_CONCURRENT_IMAGES: usize = 4`
- `pub enum ImageSource<'a>`
- `pub struct HostStats`
- `pub struct ImageHost`
- `impl ImageHost`
  - `pub fn new(config: HostConfig, registry: Arc<ProviderRegistry>) -> Self`
  - `pub fn with_providers( …`
  - `pub fn from_json(json: &str, registry: Arc<ProviderRegistry>) -> Result<Self, AssetError>`
  - `pub fn with_dedup_capacity(mut self, capacity: usize) -> Self`
  - `pub fn with_breaker(mut self, breaker: Arc<CircuitBreaker>) -> Self`
  - `pub fn breaker(&self) -> &Arc<CircuitBreaker>`
  - `pub fn providers(&self) -> Vec<String>`
  - `pub fn whitelist(&self) -> &[String]`
  - `pub fn stats(&self) -> HostStats`
  - `pub fn registry(&self) -> &Arc<ProviderRegistry>`
  - `pub fn dedup(&self) -> &Arc<UploadDedupCache>`
  - `pub fn is_probe_skipped(&self, provider: &str) -> bool`
- `pub fn is_local(src: &str) -> bool`

## `src/http.rs`

- `pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>`
- `pub const MAX_REDIRECTS: usize = 5`
- `pub const FETCH_TIMEOUT_SECS: u64 = 15`
- `pub const HEAD_TIMEOUT_SECS: u64 = 8`
- `pub const MAX_FETCH_BYTES: usize = 20 << 20`
- `pub const ERROR_BODY_LIMIT: usize = 200`
- `pub enum HttpMethod`
- `impl HttpMethod`
  - `pub const fn as_str(self) -> &'static str`
- `pub enum MultipartEntry`
- `pub struct Multipart`
- `impl Multipart`
  - `pub fn new() -> Self`
  - `pub fn field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self`
  - `pub fn file( …`
  - `pub fn entries(&self) -> &[MultipartEntry]`
  - `pub fn field_value(&self, name: &str) -> Option<&str>`
  - `pub fn file_part(&self, name: &str) -> Option<FilePart<'_>>`
- `pub struct FilePart<'a>`
- `pub enum HttpBody`
- `pub struct HttpRequest`
- `impl HttpRequest`
  - `pub fn new(method: HttpMethod, url: impl Into<String>) -> Self`
  - `pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self`
  - `pub fn with_headers(mut self, headers: &[(&str, &str)]) -> Self`
  - `pub fn bytes(mut self, data: impl Into<Vec<u8>>) -> Self`
  - `pub fn form(mut self, pairs: Vec<(String, String)>) -> Self`
  - `pub fn multipart(mut self, form: Multipart) -> Self`
  - `pub fn json(mut self, value: &serde_json::Value) -> Result<Self, AssetError>`
  - `pub fn header_value(&self, name: &str) -> Option<&str>`
  - `pub fn body_bytes(&self) -> Option<&[u8]>`
  - `pub fn form_pairs(&self) -> Option<&[(String, String)]>`
  - `pub fn form_value(&self, name: &str) -> Option<&str>`
  - `pub fn multipart_form(&self) -> Option<&Multipart>`
- `pub struct HttpResponse`
- `impl HttpResponse`
  - `pub fn new(status: u16) -> Self`
  - `pub fn ok() -> Self`
  - `pub fn with_status(status: u16, body: impl Into<Vec<u8>>) -> Self`
  - `pub fn json(body: impl Into<String>) -> Self`
  - `pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self`
  - `pub fn with_final_url(mut self, url: impl Into<String>) -> Self`
  - `pub fn header(&self, name: &str) -> Option<&str>`
  - `pub fn text(&self) -> String`
- `pub trait HttpClient: Send + Sync`
- `pub struct ReqwestClient`
- `impl ReqwestClient`
  - `pub fn new() -> Self`
  - `pub fn with_client(client: reqwest::Client) -> Self`
  - `pub fn inner(&self) -> &reqwest::Client`
- `pub fn parse_json<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, AssetError>`
- `pub fn truncate_for_log(raw: &[u8]) -> String`
- `pub fn file_base_name(name: &str) -> String`

## `src/input.rs`

- `pub struct MediaInput`
- `pub enum MediaSource<'a>`
- `pub fn decode(source: MediaSource<'_>) -> Result<MediaInput, AssetError>`
- `pub fn decode_image(source: MediaSource<'_>) -> Result<MediaInput, AssetError>`
- `pub fn has_ext(name: &str) -> bool`
- `pub fn base_name(path: &str) -> String`
- `pub fn filename_for(mime: &str, prefix: &str) -> String`
- `pub fn probe_mime_of(data: &[u8]) -> String`

## `src/lib.rs`

- `pub use breaker::{ BreakerClock, BreakerConfig, BreakerEvent, BreakerSnapshot, BreakerVerdict, CircuitBreaker, DEFAULT_COOLDOWN_MAX, DEFAULT_COOLDOWN_MIN, DEFAULT_FAIL_THRESHOLD, }`
- `pub use cache::{DedupEntry, UploadDedupCache}`
- `pub use config::{HostConfig, ProviderItem}`
- `pub use error::{AssetError, AssetResult}`
- `pub use host::{is_local, HostStats, ImageHost, ImageSource, MAX_CONCURRENT_IMAGES}`
- `pub use http::{ BoxFuture, HttpClient, HttpMethod, HttpRequest, HttpResponse, Multipart, ReqwestClient, }`
- `pub use input::{decode, decode_image, MediaInput, MediaSource}`
- `pub use official::{official_upload, set_official_upload, OfficialUploadFn, OfficialUploadRequest}`
- `pub use provider::{MediaKind, MediaKinds, Provider, ResolvedImage, UploadContext, UploadInput}`
- `pub use registry::{ConfigField, ProviderEntry, ProviderFactory, ProviderRegistry, ProviderSpec}`
- `pub use sign::{bkn, cos_sign, urlencode_go_compat}`
- `pub const CRATE_NAME: &str = "aurorix-assets"`

## `src/markdown.rs`

- `pub struct ImageRef<'a>`
- `pub fn find_images(content: &str) -> Vec<ImageRef<'_>>`
- `pub fn has_dimensions(alt: &str) -> bool`
- `pub fn render_image(alt: &str, src: &str, resolved: &ResolvedImage) -> String`

## `src/mime.rs`

- `pub const PNG_SIGNATURE: [u8; 4] = [0x89, b'P', b'N', b'G']`
- `pub fn is_image(buf: &[u8]) -> bool`
- `pub fn probe_image_mime(buf: &[u8]) -> &'static str`
- `pub fn probe_video_mime(buf: &[u8]) -> &'static str`
- `pub fn probe_media_mime(buf: &[u8]) -> &'static str`
- `pub fn ext_from_mime(mime: &str) -> &'static str`
- `pub fn media_ext(mime: &str) -> &'static str`
- `pub fn probe_size(buf: &[u8]) -> Option<(u32, u32)>`
- `pub fn content_type_matches(kind: crate::provider::MediaKind, content_type: &str) -> bool`

## `src/official.rs`

- `pub struct OfficialUploadRequest`
- `pub type OfficialUploadFn = Arc<`
- `pub fn set_official_upload(upload: Option<OfficialUploadFn>)`
- `pub fn official_upload() -> Option<OfficialUploadFn>`

## `src/provider.rs`

- `pub use crate::http::BoxFuture`
- `pub enum MediaKind`
- `impl MediaKind`
  - `pub const ALL: [MediaKind; 3] = [MediaKind::Image, MediaKind::Video, MediaKind::File]`
  - `pub const fn bit(self) -> u8`
  - `pub const fn label(self) -> &'static str`
  - `pub fn of_mime(mime: &str) -> Self`
- `pub struct MediaKinds(u8)`
- `impl MediaKinds`
  - `pub const NONE: MediaKinds = MediaKinds(0)`
  - `pub const IMAGE: MediaKinds = MediaKinds(1)`
  - `pub const VIDEO: MediaKinds = MediaKinds(2)`
  - `pub const FILE: MediaKinds = MediaKinds(4)`
  - `pub const fn all() -> Self`
  - `pub fn from_bits(bits: u8) -> Self`
  - `pub fn contains(self, kind: MediaKind) -> bool`
  - `pub fn is_empty(self) -> bool`
  - `pub fn bits(self) -> u8`
- `pub struct UploadInput<'a>`
- `impl UploadInput<'a>`
  - `pub fn new(bytes: &'a [u8], filename: impl Into<String>, mime: impl Into<String>) -> Self`
  - `pub fn with_target(mut self, target: Target) -> Self`
  - `pub fn with_trace(mut self, trace: impl Into<String>) -> Self`
  - `pub fn with_command(mut self, command: impl Into<String>) -> Self`
  - `pub fn size(&self) -> u64`
- `pub struct UploadContext<'a>`
- `impl UploadContext<'a>`
  - `pub fn new(target: Target) -> Self`
  - `pub fn with_trace(mut self, trace: &'a str) -> Self`
  - `pub fn with_command(mut self, command: &'a str) -> Self`
- `pub struct ResolvedImage`
- `impl ResolvedImage`
  - `pub fn passthrough(url: impl Into<String>) -> Self`
  - `pub fn uploaded( …`
  - `pub fn with_size(mut self, width: u32, height: u32) -> Self`
  - `pub fn is_passthrough(&self) -> bool`
- `pub trait Provider: Send + Sync`

## `src/providers/announce.rs`

- `pub const NAME: &str = "announce"`
- `pub struct AnnounceProvider`

## `src/providers/bilibili.rs`

- `pub const NAME: &str = "bilibili"`
- `pub struct BilibiliProvider`

## `src/providers/chatglm.rs`

- `pub const NAME: &str = "chatglm"`
- `pub struct ChatglmProvider`

## `src/providers/chinaunicom.rs`

- `pub const NAME: &str = "chinaunicom"`
- `pub struct UnicomProvider`

## `src/providers/cnb.rs`

- `pub const NAME: &str = "cnb"`
- `pub struct CnbProvider`

## `src/providers/homework.rs`

- `pub const NAME: &str = "homework"`
- `pub struct HomeworkProvider`

## `src/providers/kurobbs.rs`

- `pub const NAME: &str = "kurobbs"`
- `pub struct KurobbsProvider`

## `src/providers/mihoyo.rs`

- `pub const NAME: &str = "mihoyo"`
- `pub struct MihoyoProvider`
- `impl MihoyoProvider`

## `src/providers/mod.rs`

- `pub fn builtin_entries() -> Vec<ProviderEntry>`

## `src/providers/qqbot.rs`

- `pub const NAME: &str = "qqbot"`
- `pub struct QqbotProvider`

## `src/providers/qqgame.rs`

- `pub const NAME: &str = "qqgame"`
- `pub struct QqgameProvider`

## `src/providers/qqstream.rs`

- `pub const NAME: &str = "qqstream"`
- `pub struct QqstreamProvider`

## `src/providers/qzone.rs`

- `pub const NAME: &str = "qzone"`
- `pub struct QzoneProvider`
- `impl QzoneProvider`

## `src/providers/sgame.rs`

- `pub const NAME: &str = "sgame"`
- `pub struct SgameProvider`

## `src/providers/util.rs`

- `pub fn cfg_str(config: &Map<String, Value>, key: &str, default: &str) -> String`
- `pub fn cfg_required(config: &Map<String, Value>, key: &str) -> Option<String>`
- `pub fn cfg_int(config: &Map<String, Value>, key: &str, default: i64) -> i64`
- `pub fn skey_from_cookie(cookie: &str) -> String`
- `pub fn mime_ext(mime: &str) -> String`
- `pub fn mime_or(mime: &str) -> String`
- `pub fn truncate(text: &str, limit: usize) -> String`
- `pub fn err_no_data(provider: &str) -> AssetError`
- `pub fn wrap(context: &str, err: AssetError) -> AssetError`
- `pub fn parse_body<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, AssetError>`
- `pub fn regex_origin_url(raw: &[u8]) -> String`
- `pub fn now_unix() -> i64`
- `pub fn now_millis() -> u64`
- `pub fn merge_query(url: &str, form: &Map<String, Value>) -> String`

## `src/registry.rs`

- `pub type ProviderFactory = Box<`
- `pub struct ConfigField`
- `impl ConfigField`
  - `pub fn new(key: &str, label: &str, field_type: &str) -> Self`
  - `pub fn password(key: &str, label: &str) -> Self`
  - `pub fn text(key: &str, label: &str) -> Self`
  - `pub fn number(key: &str, label: &str) -> Self`
  - `pub fn description(mut self, description: &str) -> Self`
  - `pub fn required(mut self) -> Self`
  - `pub fn placeholder(mut self, placeholder: &str) -> Self`
  - `pub fn default_value(mut self, value: impl Into<Value>) -> Self`
  - `pub fn options(mut self, options: &[&str]) -> Self`
- `pub struct ProviderSpec`
- `pub struct ProviderEntry`
- `impl ProviderEntry`
  - `pub fn new( …`
  - `pub fn default_on(mut self) -> Self`
  - `pub fn probe_skip(mut self) -> Self`
- `pub struct ProviderRegistry`
- `impl ProviderRegistry`
  - `pub fn with_builtin() -> Self`
  - `pub fn with_client(http: Arc<dyn HttpClient>) -> Self`
  - `pub fn empty(http: Arc<dyn HttpClient>) -> Self`
  - `pub fn register(&mut self, entry: ProviderEntry)`
  - `pub fn client(&self) -> &Arc<dyn HttpClient>`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
  - `pub fn contains(&self, name: &str) -> bool`
  - `pub fn names(&self) -> Vec<&str>`
  - `pub fn specs(&self) -> Vec<ProviderSpec>`
  - `pub fn schema(&self, name: &str) -> Vec<ConfigField>`
  - `pub fn default_on_names(&self) -> Vec<&str>`
  - `pub fn is_default_on(&self, name: &str) -> bool`
  - `pub fn probe_skipped_by_default(&self, name: &str) -> bool`
  - `pub fn instantiate(&self, item: &ProviderItem) -> Result<Arc<dyn Provider>, AssetError>`

## `src/sign.rs`

- `pub fn bkn(skey: &str) -> String`
- `pub fn urlencode_go_compat(value: &str) -> String`
- `pub fn sha1_hex(data: &str) -> String`
- `pub fn hmac_sha1_hex(key: &str, data: &str) -> String`
- `pub fn md5_hex(bytes: &[u8]) -> String`
- `pub fn md5_hex_upper(bytes: &[u8]) -> String`
- `pub fn md5_base64(bytes: &[u8]) -> String`
- `pub fn base64_encode(bytes: &[u8]) -> String`
- `pub fn base64_decode(value: &str) -> Option<Vec<u8>>`
- `pub fn uuid_hex() -> String`
- `pub fn cos_canonical( …`
- `pub fn cos_sign( …`

## `src/testkit.rs`

- `pub struct MockHttp`
- `impl MockHttp`
  - `pub fn new(responses: Vec<HttpResponse>) -> Self`
  - `pub fn shared(responses: Vec<HttpResponse>) -> Arc<Self>`
  - `pub fn count(&self) -> usize`
  - `pub fn request(&self, index: usize) -> HttpRequest`
  - `pub fn last(&self) -> HttpRequest`
- `pub struct TestProvider`
- `impl TestProvider`
  - `pub fn new(name: &str) -> Self`
  - `pub fn failing(name: &str, message: &str) -> Self`
  - `pub fn always_failing(name: &str, message: &str) -> Self`
  - `pub fn failing_times(name: &str, message: &str, times: usize) -> Self`
  - `pub fn with_kinds(mut self, kinds: MediaKinds) -> Self`
  - `pub fn with_max_size(mut self, limit: u64) -> Self`
  - `pub fn with_session_scoped(mut self) -> Self`
  - `pub fn with_probe(mut self) -> Self`
  - `pub fn calls(&self) -> u32`
- `pub trait ResultExt<T>`
- `pub fn official_lock() -> MutexGuard<'static, ()>`
- `pub fn png_bytes(width: u32, height: u32) -> Vec<u8>`
- `pub struct FakeClock`
- `impl FakeClock`
  - `pub fn new() -> Self`
  - `pub fn shared() -> Arc<Self>`
  - `pub fn advance(&self, delta: Duration)`

