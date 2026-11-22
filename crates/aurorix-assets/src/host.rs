use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aurorix_log::{module, Level};
use tokio::sync::{Semaphore, SemaphorePermit};

use crate::breaker::{BreakerConfig, BreakerEvent, BreakerVerdict, CircuitBreaker};
use crate::cache::{CacheLookup, UploadDedupCache};
use crate::config::{HostConfig, ProviderItem};
use crate::error::AssetError;
use crate::http::{
    HttpClient, HttpMethod, HttpRequest, FETCH_TIMEOUT_SECS, HEAD_TIMEOUT_SECS, MAX_FETCH_BYTES,
};
use crate::input::{base_name, decode_image, has_ext, MediaSource};
use crate::markdown;
use crate::mime::{
    content_type_matches, media_ext, probe_image_mime, probe_media_mime, probe_size,
};
use crate::provider::{MediaKind, Provider, ResolvedImage, UploadContext, UploadInput};
use crate::registry::ProviderRegistry;
use crate::sign::md5_hex;

pub const MAX_CONCURRENT_IMAGES: usize = 4;

#[derive(Clone, Copy, Debug)]
pub enum ImageSource<'a> {
    Str(&'a str),
    Bytes(&'a [u8]),
}

impl<'a> From<&'a str> for ImageSource<'a> {
    fn from(value: &'a str) -> Self {
        ImageSource::Str(value)
    }
}

impl<'a> From<&'a [u8]> for ImageSource<'a> {
    fn from(value: &'a [u8]) -> Self {
        ImageSource::Bytes(value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostStats {
    pub uploaded: u64,
    pub cache_hits: u64,
    pub providers: usize,
    pub whitelist: usize,
}

pub struct ImageHost {
    providers: Vec<Arc<dyn Provider>>,
    skip_probe: HashSet<String>,
    whitelist: Vec<String>,
    registry: Arc<ProviderRegistry>,
    semaphore: Arc<Semaphore>,
    dedup: Arc<UploadDedupCache>,
    breaker: Arc<CircuitBreaker>,
    uploaded: AtomicU64,
    cache_hits: AtomicU64,
}

impl ImageHost {
    pub fn new(config: HostConfig, registry: Arc<ProviderRegistry>) -> Self {
        let configured: HashSet<String> = config
            .providers
            .iter()
            .map(|item| item.name.clone())
            .collect();
        let mut enabled: Vec<ProviderItem> = config
            .providers
            .into_iter()
            .filter(ProviderItem::is_enabled)
            .collect();
        enabled.sort_by_key(|item| Reverse(item.priority));

        let mut providers: Vec<Arc<dyn Provider>> = Vec::new();
        let mut skip_probe: HashSet<String> = HashSet::new();
        for item in &enabled {
            let Ok(provider) = registry.instantiate(item) else {
                continue;
            };
            let skipped = match item.probe {
                Some(flag) => !flag,
                None => registry.probe_skipped_by_default(&item.name),
            };
            if skipped {
                skip_probe.insert(provider.name().to_string());
            }
            providers.push(provider);
        }

        for name in registry.default_on_names() {
            if configured.contains(name) {
                continue;
            }
            let item = ProviderItem::new(name);
            let Ok(provider) = registry.instantiate(&item) else {
                continue;
            };
            if registry.probe_skipped_by_default(name) {
                skip_probe.insert(provider.name().to_string());
            }
            providers.push(provider);
        }

        Self::assemble(providers, skip_probe, config.whitelist, registry)
    }

    pub fn with_providers(
        providers: Vec<Arc<dyn Provider>>,
        whitelist: Vec<String>,
        http: Arc<dyn HttpClient>,
    ) -> Self {
        let skip_probe = providers
            .iter()
            .filter(|provider| provider.skip_probe())
            .map(|provider| provider.name().to_string())
            .collect();
        let registry = Arc::new(ProviderRegistry::empty(http));
        Self::assemble(providers, skip_probe, whitelist, registry)
    }

    fn assemble(
        providers: Vec<Arc<dyn Provider>>,
        skip_probe: HashSet<String>,
        whitelist: Vec<String>,
        registry: Arc<ProviderRegistry>,
    ) -> Self {
        Self {
            providers,
            skip_probe,
            whitelist,
            registry,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_IMAGES)),
            dedup: Arc::new(UploadDedupCache::default()),
            breaker: Arc::new(CircuitBreaker::new(BreakerConfig::default())),
            uploaded: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
        }
    }

    pub fn from_json(json: &str, registry: Arc<ProviderRegistry>) -> Result<Self, AssetError> {
        let config: HostConfig = serde_json::from_str(json)
            .map_err(|err| AssetError::Json(format!("assets.json 解析失败: {err}")))?;
        Ok(Self::new(config, registry))
    }

    pub fn with_dedup_capacity(mut self, capacity: usize) -> Self {
        self.dedup = Arc::new(UploadDedupCache::new(capacity));
        self
    }

    pub fn with_breaker(mut self, breaker: Arc<CircuitBreaker>) -> Self {
        self.breaker = breaker;
        self
    }

    pub fn breaker(&self) -> &Arc<CircuitBreaker> {
        &self.breaker
    }

    pub fn providers(&self) -> Vec<String> {
        self.providers
            .iter()
            .map(|provider| provider.name().to_string())
            .collect()
    }

    pub fn whitelist(&self) -> &[String] {
        &self.whitelist
    }

    pub fn stats(&self) -> HostStats {
        HostStats {
            uploaded: self.uploaded.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            providers: self.providers.len(),
            whitelist: self.whitelist.len(),
        }
    }

    pub fn registry(&self) -> &Arc<ProviderRegistry> {
        &self.registry
    }

    pub fn dedup(&self) -> &Arc<UploadDedupCache> {
        &self.dedup
    }

    pub fn is_probe_skipped(&self, provider: &str) -> bool {
        self.skip_probe.contains(provider)
    }

    pub async fn resolve(
        &self,
        source: ImageSource<'_>,
        ctx: UploadContext<'_>,
    ) -> Result<ResolvedImage, AssetError> {
        match source {
            ImageSource::Str(src) => self.resolve_str(src, &ctx).await,
            ImageSource::Bytes(bytes) => self.resolve_bytes(bytes, "", "", ctx).await,
        }
    }

    pub async fn resolve_bytes(
        &self,
        bytes: &[u8],
        filename: &str,
        mime: &str,
        ctx: UploadContext<'_>,
    ) -> Result<ResolvedImage, AssetError> {
        if bytes.is_empty() {
            return Err(AssetError::message("空媒体数据"));
        }
        if !crate::mime::is_image(bytes) {
            return Err(AssetError::message("字节非图片"));
        }
        let mut mime = mime.to_string();
        if mime.is_empty() {
            mime = probe_image_mime(bytes).to_string();
        }
        let mut filename = filename.to_string();
        if filename.is_empty() || !has_ext(&filename) {
            let ext = crate::mime::ext_from_mime(&mime);
            if !ext.is_empty() {
                filename = format!("inline.{ext}");
            }
        }
        let _permit = self.acquire().await?;
        let input = UploadInput::new(bytes, filename, mime)
            .with_target(ctx.target.clone())
            .with_trace(ctx.trace)
            .with_command(ctx.command);
        let (url, provider, session_scoped) = self.upload_to(&input, MediaKind::Image).await?;
        let (width, height) = probe_size(bytes).unwrap_or((0, 0));
        Ok(ResolvedImage {
            url,
            width,
            height,
            provider,
            session_scoped,
        })
    }

    pub async fn resolve_media(
        &self,
        bytes: &[u8],
        kind: MediaKind,
        ctx: UploadContext<'_>,
    ) -> Result<ResolvedImage, AssetError> {
        if bytes.is_empty() {
            return Err(AssetError::message("空媒体数据"));
        }
        let mime = probe_media_mime(bytes).to_string();
        let ext = media_ext(&mime);
        let filename = if ext.is_empty() {
            "media".to_string()
        } else {
            format!("media.{ext}")
        };
        let _permit = self.acquire().await?;
        let input = UploadInput::new(bytes, filename, mime)
            .with_target(ctx.target.clone())
            .with_trace(ctx.trace)
            .with_command(ctx.command);
        let (url, provider, session_scoped) = self.upload_to(&input, kind).await?;
        Ok(ResolvedImage::uploaded(url, provider, session_scoped))
    }

    pub async fn process_markdown_with(&self, content: &str, ctx: UploadContext<'_>) -> String {
        if self.providers.is_empty() {
            return content.to_string();
        }
        let images = markdown::find_images(content);
        if images.is_empty() {
            return content.to_string();
        }
        let mut out = String::with_capacity(content.len());
        let mut last = 0usize;
        for image in images {
            out.push_str(&content[last..image.start]);
            let rendered = match self.resolve_str(image.src, &ctx).await {
                Ok(resolved) => markdown::render_image(image.alt, image.src, &resolved),
                Err(_) => format!("![{}]({})", image.alt, image.src),
            };
            out.push_str(&rendered);
            last = image.end;
        }
        out.push_str(&content[last..]);
        out
    }

    async fn resolve_str(
        &self,
        src: &str,
        ctx: &UploadContext<'_>,
    ) -> Result<ResolvedImage, AssetError> {
        if self.whitelisted(src) {
            return Ok(ResolvedImage::passthrough(src));
        }
        if self.providers.is_empty() || !is_local(src) {
            return Ok(ResolvedImage::passthrough(src));
        }
        let _permit = self.acquire().await?;
        let Ok((data, filename, mime)) = self.load(src).await else {
            return Ok(ResolvedImage::passthrough(src));
        };
        let input = UploadInput::new(&data, filename, mime)
            .with_target(ctx.target.clone())
            .with_trace(ctx.trace)
            .with_command(ctx.command);
        let (url, provider, session_scoped) = self.upload_to(&input, MediaKind::Image).await?;
        let (width, height) = probe_size(&data).unwrap_or((0, 0));
        Ok(ResolvedImage {
            url,
            width,
            height,
            provider,
            session_scoped,
        })
    }

    fn whitelisted(&self, src: &str) -> bool {
        self.whitelist
            .iter()
            .any(|prefix| src.starts_with(prefix.as_str()))
    }

    async fn acquire(&self) -> Result<SemaphorePermit<'_>, AssetError> {
        self.semaphore
            .acquire()
            .await
            .map_err(|_| AssetError::message("图床并发闸门已关闭"))
    }

    async fn load(&self, src: &str) -> Result<(Vec<u8>, String, String), AssetError> {
        let input = decode_image(MediaSource::Str(src))?;
        if !input.url.is_empty() {
            let (data, mime) = self.fetch_remote(&input.url).await?;
            return Ok((data, base_name(&input.url), mime));
        }
        Ok((input.data, input.filename, input.mime))
    }

    async fn fetch_remote(&self, url: &str) -> Result<(Vec<u8>, String), AssetError> {
        let request = HttpRequest::new(HttpMethod::Get, url);
        let outcome = tokio::time::timeout(
            Duration::from_secs(FETCH_TIMEOUT_SECS),
            self.registry.client().execute(request),
        )
        .await;
        let response = match outcome {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => return Err(AssetError::message(format!("download image: {err}"))),
            Err(_) => return Err(AssetError::message("download image: 请求超时")),
        };
        if response.status != 200 {
            let status = response.status;
            return Err(AssetError::message(format!(
                "download image: status {status}"
            )));
        }
        if response.body.len() > MAX_FETCH_BYTES {
            return Err(AssetError::message(format!(
                "image exceeds {MAX_FETCH_BYTES} bytes"
            )));
        }
        let mime = response.header("content-type").unwrap_or("").to_string();
        Ok((response.body, mime))
    }

    async fn probe_url(&self, raw: &str) -> (String, String, bool) {
        if !raw.starts_with("http://") && !raw.starts_with("https://") {
            return (String::new(), String::new(), false);
        }
        let request = HttpRequest::new(HttpMethod::Head, raw);
        let outcome = tokio::time::timeout(
            Duration::from_secs(HEAD_TIMEOUT_SECS),
            self.registry.client().execute(request),
        )
        .await;
        let Ok(Ok(response)) = outcome else {
            return (String::new(), String::new(), false);
        };
        let final_url = if response.final_url.is_empty() {
            raw.to_string()
        } else {
            response.final_url.clone()
        };
        let content_type = response.header("content-type").unwrap_or("").to_string();
        (final_url, content_type, true)
    }

    fn candidates_for(
        &self,
        input: &UploadInput<'_>,
        kind: MediaKind,
    ) -> Result<Vec<Arc<dyn Provider>>, AssetError> {
        if self.providers.is_empty() {
            return Err(AssetError::NoProvider);
        }
        let size = input.size();
        let mut candidates = Vec::with_capacity(self.providers.len());
        let mut skipped = 0usize;
        for provider in &self.providers {
            if input.target.scene.guild() && provider.session_scoped() {
                skipped += 1;
                continue;
            }
            if !provider.kinds().contains(kind) {
                continue;
            }
            let limit = provider.max_size();
            if limit > 0 && size > limit {
                continue;
            }
            candidates.push(Arc::clone(provider));
        }
        if candidates.is_empty() {
            if skipped > 0 {
                return Err(AssetError::message(
                    "频道里没有可用的图床: 官方图床只能在群聊/私聊用, 请配置第三方图床",
                ));
            }
            return Err(AssetError::message(format!(
                "没有图床能接收 {} 类型({size} 字节)",
                kind.label()
            )));
        }
        Ok(candidates)
    }

    fn breaker_filter(&self, candidates: Vec<Arc<dyn Provider>>) -> Vec<Arc<dyn Provider>> {
        let mut allowed: Vec<Arc<dyn Provider>> = Vec::with_capacity(candidates.len());
        let mut blocked: Vec<String> = Vec::new();
        for provider in &candidates {
            let name = provider.name();
            match self.breaker.verdict(name) {
                BreakerVerdict::Allow => allowed.push(Arc::clone(provider)),
                BreakerVerdict::Probe => {
                    warn_asset(
                        "图床熔断冷却到期, 开始半开探测",
                        &[("provider", name.to_string())],
                    );
                    allowed.push(Arc::clone(provider));
                }
                BreakerVerdict::Blocked => blocked.push(name.to_string()),
            }
        }
        if allowed.is_empty() {
            let fallback = Arc::clone(&candidates[0]);
            warn_asset(
                "所有图床均在熔断冷却中, 兜底放行最优先图床",
                &[
                    ("provider", fallback.name().to_string()),
                    ("skipped", blocked.join(",")),
                ],
            );
            return vec![fallback];
        }
        if !blocked.is_empty() {
            warn_asset(
                "跳过处于熔断冷却的图床",
                &[
                    ("skipped", blocked.join(",")),
                    (
                        "remaining",
                        candidates.len().saturating_sub(blocked.len()).to_string(),
                    ),
                ],
            );
        }
        allowed
    }

    async fn upload_to(
        &self,
        input: &UploadInput<'_>,
        kind: MediaKind,
    ) -> Result<(String, String, bool), AssetError> {
        let candidates = self.breaker_filter(self.candidates_for(input, kind)?);
        let cache_key = if kind == MediaKind::Image && !input.bytes.is_empty() {
            Some(md5_hex(input.bytes))
        } else {
            None
        };
        if let Some(key) = cache_key.as_deref() {
            match self.dedup.lookup(key) {
                CacheLookup::Fresh(hit) => {
                    self.cache_hits.fetch_add(1, Ordering::Relaxed);
                    let session_scoped = self.session_scoped(&hit.provider);
                    return Ok((hit.url, hit.provider, session_scoped));
                }
                CacheLookup::Probe(hit) => {
                    let alive =
                        self.skip_probe.contains(&hit.provider) || self.probe_url(&hit.url).await.2;
                    if alive {
                        self.cache_hits.fetch_add(1, Ordering::Relaxed);
                        let session_scoped = self.session_scoped(&hit.provider);
                        return Ok((hit.url, hit.provider, session_scoped));
                    }
                    self.dedup.invalidate(key);
                }
                CacheLookup::Stale(hit) => {
                    let alive = !self.skip_probe.contains(&hit.provider)
                        && self.probe_url(&hit.url).await.2;
                    if alive && self.dedup.renew(key) {
                        self.cache_hits.fetch_add(1, Ordering::Relaxed);
                        let session_scoped = self.session_scoped(&hit.provider);
                        return Ok((hit.url, hit.provider, session_scoped));
                    }
                    self.dedup.invalidate(key);
                }
                CacheLookup::Miss => {}
            }
        }
        let names: Vec<String> = candidates
            .iter()
            .map(|provider| provider.name().to_string())
            .collect();
        let mut errors: Vec<String> = Vec::new();
        for (index, provider) in candidates.into_iter().enumerate() {
            let name = names[index].clone();
            let next = names.get(index + 1).cloned().unwrap_or_default();
            let attempt = input.clone();
            let url = match provider.upload(attempt).await {
                Ok(url) => url,
                Err(err) => {
                    self.note_failure(&name, &next, &err.to_string());
                    errors.push(format!("[{name}] {err}"));
                    continue;
                }
            };
            let mut final_url = url;
            let skip_probe = self.skip_probe.contains(&name);
            if !skip_probe {
                let (probed_url, content_type, head_ok) = self.probe_url(&final_url).await;
                if !probed_url.is_empty() {
                    final_url = probed_url;
                }
                if head_ok && !content_type_matches(kind, &content_type) {
                    let detail =
                        format!("final url content-type \"{content_type}\" is not image/*");
                    self.note_failure(&name, &next, &detail);
                    errors.push(format!("[{name}] {detail}"));
                    continue;
                }
            }
            self.breaker.record_success(&name);
            self.uploaded.fetch_add(1, Ordering::Relaxed);
            let session_scoped = provider.session_scoped();
            if !session_scoped {
                if let Some(key) = cache_key.clone() {
                    self.dedup.put(key, final_url.clone(), name.clone());
                }
            }
            return Ok((final_url, name, session_scoped));
        }
        Err(AssetError::message(format!(
            "all providers failed: {}",
            errors.join("\n")
        )))
    }

    fn session_scoped(&self, name: &str) -> bool {
        self.providers
            .iter()
            .find(|provider| provider.name() == name)
            .is_some_and(|provider| provider.session_scoped())
    }

    fn note_failure(&self, provider: &str, next: &str, reason: &str) {
        match self.breaker.record_failure(provider) {
            BreakerEvent::Opened {
                cooldown,
                open_count,
            } => warn_asset(
                "图床连续失败, 进入熔断冷却",
                &[
                    ("provider", provider.to_string()),
                    (
                        "fail_streak",
                        self.breaker.fail_streak(provider).to_string(),
                    ),
                    ("cooldown_ms", cooldown.as_millis().to_string()),
                    ("open_count", open_count.to_string()),
                    ("reason", reason.to_string()),
                ],
            ),
            BreakerEvent::Streak { streak } => warn_asset(
                "图床上传失败, 降级到下一优先级",
                &[
                    ("provider", provider.to_string()),
                    ("fail_streak", streak.to_string()),
                    ("next", next.to_string()),
                    ("reason", reason.to_string()),
                ],
            ),
            BreakerEvent::Closed | BreakerEvent::AlreadyOpen => warn_asset(
                "图床上传失败",
                &[
                    ("provider", provider.to_string()),
                    ("next", next.to_string()),
                    ("reason", reason.to_string()),
                ],
            ),
        }
    }
}

fn warn_asset(message: &str, fields: &[(&str, String)]) {
    let mut map = BTreeMap::new();
    for (key, value) in fields {
        map.insert((*key).to_string(), value.clone());
    }
    module("assets").log_fields(Level::Warn, message, map);
}

pub fn is_local(src: &str) -> bool {
    if src.starts_with("data:") || src.starts_with("file://") {
        return true;
    }
    if !src.starts_with("http://") && !src.starts_with("https://") {
        return true;
    }
    let Some((_, rest)) = src.split_once("://") else {
        return true;
    };
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".local") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback() || is_private(ip) || ip.is_unspecified(),
        Err(_) => false,
    }
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => {
            let segment = v6.segments()[0];
            (segment & 0xfe00) == 0xfc00
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::BoxFuture;
    use crate::http::HttpResponse;
    use crate::provider::MediaKinds;
    use crate::provider::UploadInput as TestUploadInput;
    use crate::registry::ProviderEntry;
    use crate::testkit::ResultExt as _;
    use crate::testkit::{png_bytes, MockHttp, TestProvider};
    use aurorix_core::{Scene, Target};

    fn registry_with(names: &[&'static str]) -> ProviderRegistry {
        let mut registry = ProviderRegistry::empty(MockHttp::shared(Vec::new()));
        for name in names {
            registry.register(ProviderEntry::new(
                name,
                name,
                Vec::new(),
                Box::new(|item, _http| Ok(Arc::new(TestProvider::new(item.name.as_str())))),
            ));
        }
        registry
    }

    fn host_with(config: HostConfig, registry: ProviderRegistry) -> ImageHost {
        ImageHost::new(config, Arc::new(registry))
    }

    fn ctx() -> UploadContext<'static> {
        UploadContext::new(Target::new(Scene::Group, "g1"))
    }

    fn data_url(width: u32, height: u32) -> String {
        format!(
            "data:image/png;base64,{}",
            crate::sign::base64_encode(&png_bytes(width, height))
        )
    }

    #[test]
    fn providers_sorted_by_priority_descending_and_stable() {
        let config = HostConfig {
            providers: vec![
                ProviderItem::new("third").with_priority(1),
                ProviderItem::new("first").with_priority(30),
                ProviderItem::new("second").with_priority(30),
                ProviderItem::new("fourth").with_priority(20),
            ],
            whitelist: Vec::new(),
        };
        let host = host_with(
            config,
            registry_with(&["first", "second", "third", "fourth"]),
        );
        assert_eq!(host.providers(), vec!["first", "second", "fourth", "third"]);
    }

    #[test]
    fn disabled_providers_are_filtered_out() {
        let config = HostConfig {
            providers: vec![
                ProviderItem::new("alpha").with_priority(10),
                ProviderItem::new("beta")
                    .with_enabled(false)
                    .with_priority(99),
                ProviderItem::new("gamma")
                    .with_enabled(true)
                    .with_priority(5),
            ],
            whitelist: Vec::new(),
        };
        let host = host_with(config, registry_with(&["alpha", "beta", "gamma"]));
        assert_eq!(host.providers(), vec!["alpha", "gamma"]);
    }

    #[test]
    fn providers_that_fail_to_construct_are_skipped() {
        let mut registry = ProviderRegistry::empty(MockHttp::shared(Vec::new()));
        registry.register(ProviderEntry::new(
            "needs_token",
            "需要令牌",
            Vec::new(),
            Box::new(|item, _http| {
                if item.config.contains_key("token") {
                    Ok(Arc::new(TestProvider::new("needs_token")))
                } else {
                    Err(AssetError::message("needs_token: token 必填"))
                }
            }),
        ));
        registry.register(ProviderEntry::new(
            "always",
            "总是可用",
            Vec::new(),
            Box::new(|_item, _http| Ok(Arc::new(TestProvider::new("always")))),
        ));
        let config = HostConfig {
            providers: vec![
                ProviderItem::new("needs_token").with_priority(50),
                ProviderItem::new("always").with_priority(1),
            ],
            whitelist: Vec::new(),
        };
        let host = host_with(config, registry);
        assert_eq!(host.providers(), vec!["always"]);

        let mut registry = ProviderRegistry::empty(MockHttp::shared(Vec::new()));
        registry.register(ProviderEntry::new(
            "needs_token",
            "需要令牌",
            Vec::new(),
            Box::new(|item, _http| {
                if item.config.contains_key("token") {
                    Ok(Arc::new(TestProvider::new("needs_token")))
                } else {
                    Err(AssetError::message("needs_token: token 必填"))
                }
            }),
        ));
        let config = HostConfig {
            providers: vec![ProviderItem::new("needs_token").with_config("token", "t")],
            whitelist: Vec::new(),
        };
        let host = host_with(config, registry);
        assert_eq!(host.providers(), vec!["needs_token"]);
    }

    #[test]
    fn default_providers_are_appended_at_the_tail() {
        let registry = ProviderRegistry::with_client(MockHttp::shared(Vec::new()));
        let host = ImageHost::new(HostConfig::default(), Arc::new(registry));
        assert_eq!(host.providers(), vec!["qqbot"]);
        assert!(host.is_probe_skipped("qqbot"));

        let registry = ProviderRegistry::with_client(MockHttp::shared(Vec::new()));
        let config = HostConfig {
            providers: vec![ProviderItem::new("chatglm").with_priority(10)],
            whitelist: Vec::new(),
        };
        let host = ImageHost::new(config, Arc::new(registry));
        assert_eq!(host.providers(), vec!["chatglm", "qqbot"]);
    }

    #[test]
    fn probe_flags_follow_config_and_provider_defaults() {
        let config = HostConfig {
            providers: vec![
                ProviderItem::new("first").with_probe(false),
                ProviderItem::new("second").with_probe(true),
                ProviderItem::new("cnb")
                    .with_config("token", "t")
                    .with_config("defaultRepo", "u/r"),
            ],
            whitelist: Vec::new(),
        };
        let mut registry = registry_with(&["first", "second"]);
        for entry in crate::providers::builtin_entries() {
            if entry.name == "cnb" {
                registry.register(entry);
            }
        }
        let host = host_with(config, registry);
        assert!(host.is_probe_skipped("first"));
        assert!(!host.is_probe_skipped("second"));
        assert!(host.is_probe_skipped("cnb"));
    }

    #[tokio::test]
    async fn whitelisted_urls_pass_through_even_when_local() {
        let config = HostConfig {
            providers: vec![ProviderItem::new("first")],
            whitelist: vec!["https://img.example.com/".to_string()],
        };
        let host = host_with(config, registry_with(&["first"]));
        let resolved = host
            .resolve(ImageSource::Str("https://img.example.com/a.png"), ctx())
            .await
            .unwrap();
        assert_eq!(resolved.url, "https://img.example.com/a.png");
        assert!(resolved.is_passthrough());
        assert_eq!(host.whitelist(), ["https://img.example.com/".to_string()]);
    }

    #[tokio::test]
    async fn public_urls_pass_through_without_uploading() {
        let provider = Arc::new(TestProvider::new("first"));
        let provider_dyn: Arc<dyn Provider> = provider.clone();
        let host =
            ImageHost::with_providers(vec![provider_dyn], Vec::new(), MockHttp::shared(Vec::new()));
        let resolved = host
            .resolve(ImageSource::Str("https://cdn.example.com/x.png"), ctx())
            .await
            .unwrap();
        assert_eq!(resolved.url, "https://cdn.example.com/x.png");
        assert_eq!(provider.calls(), 0);
    }

    #[tokio::test]
    async fn empty_chain_passes_everything_through() {
        let host = ImageHost::with_providers(Vec::new(), Vec::new(), MockHttp::shared(Vec::new()));
        let resolved = host
            .resolve(ImageSource::Str(&data_url(2, 2)), ctx())
            .await
            .unwrap();
        assert_eq!(resolved.url, data_url(2, 2));
        assert_eq!(host.providers().len(), 0);
        let stats = host.stats();
        assert_eq!(stats.providers, 0);
        assert_eq!(stats.uploaded, 0);
    }

    #[tokio::test]
    async fn failing_provider_falls_back_to_the_next_one() {
        let first = Arc::new(TestProvider::failing("first", "boom"));
        let second = Arc::new(TestProvider::new("second"));
        let host = ImageHost::with_providers(
            vec![first.clone(), second.clone()],
            Vec::new(),
            MockHttp::shared(Vec::new()),
        );
        let resolved = host
            .resolve(ImageSource::Str(&data_url(3, 4)), ctx())
            .await
            .unwrap();
        assert_eq!(resolved.provider, "second");
        assert_eq!(resolved.url, "https://cdn.example/second.png");
        assert_eq!(resolved.width, 3);
        assert_eq!(resolved.height, 4);
        assert_eq!(first.calls(), 1);
        assert_eq!(second.calls(), 1);
        assert_eq!(host.stats().uploaded, 1);
    }

    #[tokio::test]
    async fn all_providers_failing_reports_joined_errors() {
        let host = ImageHost::with_providers(
            vec![
                Arc::new(TestProvider::failing("first", "boom")),
                Arc::new(TestProvider::failing("second", "bang")),
            ],
            Vec::new(),
            MockHttp::shared(Vec::new()),
        );
        let err = host
            .resolve(ImageSource::Str(&data_url(1, 1)), ctx())
            .await
            .unwrap_err();
        let message = err.to_string();
        assert!(message.starts_with("all providers failed: "));
        assert!(message.contains("[first] boom"));
        assert!(message.contains("[second] bang"));
    }

    #[tokio::test]
    async fn probe_content_type_mismatch_switches_provider() {
        let first = Arc::new(TestProvider::new("first").with_probe());
        let second = Arc::new(TestProvider::new("second"));
        let http = MockHttp::shared(vec![
            HttpResponse::ok().with_header("Content-Type", "text/html")
        ]);
        let host = ImageHost::with_providers(vec![first.clone(), second.clone()], Vec::new(), http);
        let resolved = host
            .resolve(ImageSource::Bytes(&png_bytes(2, 2)), ctx())
            .await
            .unwrap();
        assert_eq!(resolved.provider, "second");
        assert_eq!(first.calls(), 1);
    }

    #[tokio::test]
    async fn unreachable_head_probe_keeps_the_url() {
        let provider = Arc::new(TestProvider::new("first").with_probe());
        let host =
            ImageHost::with_providers(vec![provider], Vec::new(), MockHttp::shared(Vec::new()));
        let resolved = host
            .resolve(ImageSource::Bytes(&png_bytes(2, 2)), ctx())
            .await
            .unwrap();
        assert_eq!(resolved.url, "https://cdn.example/first.png");
        assert_eq!(resolved.provider, "first");
    }

    #[tokio::test]
    async fn probe_follows_redirect_and_reports_final_url() {
        let provider = Arc::new(TestProvider::new("first").with_probe());
        let http = MockHttp::shared(vec![HttpResponse::ok()
            .with_header("Content-Type", "image/png")
            .with_final_url("https://cdn.example/final.png")]);
        let host = ImageHost::with_providers(vec![provider], Vec::new(), http);
        let resolved = host
            .resolve(ImageSource::Bytes(&png_bytes(2, 2)), ctx())
            .await
            .unwrap();
        assert_eq!(resolved.url, "https://cdn.example/final.png");
    }

    #[tokio::test]
    async fn channel_scene_skips_session_scoped_providers() {
        let host = ImageHost::with_providers(
            vec![Arc::new(TestProvider::new("qqbot").with_session_scoped())],
            Vec::new(),
            MockHttp::shared(Vec::new()),
        );
        let guild = UploadContext::new(Target::new(Scene::Channel, "c1"));
        let err = host
            .resolve(ImageSource::Bytes(&png_bytes(2, 2)), guild)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("频道里没有可用的图床"));

        let group = UploadContext::new(Target::new(Scene::Group, "g1"));
        let resolved = host
            .resolve(ImageSource::Bytes(&png_bytes(2, 2)), group)
            .await
            .unwrap();
        assert_eq!(resolved.provider, "qqbot");
        assert!(resolved.session_scoped);
    }

    #[tokio::test]
    async fn media_kind_and_size_filter_candidates() {
        let image_only = Arc::new(TestProvider::new("image_only"));
        let video = Arc::new(
            TestProvider::new("video")
                .with_kinds(MediaKinds::IMAGE | MediaKinds::VIDEO)
                .with_max_size(4),
        );
        let host = ImageHost::with_providers(
            vec![image_only.clone(), video.clone()],
            Vec::new(),
            MockHttp::shared(Vec::new()),
        );
        let big = png_bytes(2, 2);
        assert!(big.len() > 4);
        let err = host
            .resolve_media(&big, MediaKind::Video, ctx())
            .await
            .failure();
        assert!(err.to_string().contains("没有图床能接收 视频 类型"));
        let resolved = host
            .resolve_media(&big, MediaKind::Image, ctx())
            .await
            .unwrap();
        assert_eq!(resolved.provider, "image_only");
        let small = vec![0u8; 2];
        let resolved = host
            .resolve_media(&small, MediaKind::Video, ctx())
            .await
            .unwrap();
        assert_eq!(resolved.provider, "video");
    }

    #[tokio::test]
    async fn markdown_rewrite_adds_size_and_keeps_failures() {
        let provider = Arc::new(TestProvider::new("first"));
        let host =
            ImageHost::with_providers(vec![provider], Vec::new(), MockHttp::shared(Vec::new()));
        let content = format!(
            "前 ![alt]({}) 中 ![](https://cdn.example.com/x.png) 后 ![](data:image/png;base64,{})",
            data_url(12, 34),
            crate::sign::base64_encode(&png_bytes(6, 7))
        );
        let out = host.process_markdown_with(&content, ctx()).await;
        assert!(out.contains("![alt #12px #34px](https://cdn.example/first.png)"));
        assert!(out.contains("![](https://cdn.example.com/x.png)"));

        let failing = ImageHost::with_providers(
            vec![Arc::new(TestProvider::failing("bad", "boom"))],
            Vec::new(),
            MockHttp::shared(Vec::new()),
        );
        let src = data_url(1, 1);
        let content = format!("![alt]({src})");
        let out = failing.process_markdown_with(&content, ctx()).await;
        assert_eq!(out, format!("![alt]({src})"));
    }

    #[tokio::test]
    async fn dedup_cache_uploads_identical_bytes_once() {
        let http = MockHttp::shared(vec![HttpResponse::json(
            r#"{"result":{"file_url":"https://cdn.chatglm/x.png"}}"#,
        )]);
        let registry = Arc::new(ProviderRegistry::with_client(http.clone()));
        let config = HostConfig {
            providers: vec![ProviderItem::new("chatglm").with_probe(false)],
            whitelist: Vec::new(),
        };
        let host = ImageHost::new(config, registry);
        let bytes = png_bytes(5, 5);
        let first = host
            .resolve_bytes(&bytes, "a.png", "image/png", ctx())
            .await
            .unwrap();
        let second = host
            .resolve_bytes(&bytes, "b.png", "image/png", ctx())
            .await
            .unwrap();
        assert_eq!(first.url, "https://cdn.chatglm/x.png");
        assert_eq!(second.url, first.url);
        assert_eq!(http.count(), 1);
        let stats = host.stats();
        assert_eq!(stats.uploaded, 1);
        assert_eq!(stats.cache_hits, 1);
        assert_eq!(host.dedup().len(), 1);
    }

    #[tokio::test]
    async fn fresh_host_reads_config_and_resets_stats() {
        let registry = Arc::new(ProviderRegistry::with_client(MockHttp::shared(Vec::new())));
        let json = r#"{"providers":[{"name":"chatglm","priority":5}],"whitelist":["https://img.example.com/"]}"#;
        let host = ImageHost::from_json(json, registry).unwrap();
        assert_eq!(host.providers(), vec!["chatglm", "qqbot"]);
        assert_eq!(host.whitelist().len(), 1);
        assert_eq!(host.stats().uploaded, 0);
        let broken = ImageHost::from_json(
            "not json",
            Arc::new(ProviderRegistry::with_client(MockHttp::shared(Vec::new()))),
        );
        let err = match broken {
            Ok(_) => panic!("非法 JSON 必须报错"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("assets.json 解析失败"));
    }

    struct SlowProvider {
        active: AtomicU64,
        peak: AtomicU64,
    }

    impl Provider for SlowProvider {
        fn name(&self) -> &str {
            "slow"
        }

        fn skip_probe(&self) -> bool {
            true
        }

        fn upload<'a>(
            &'a self,
            _input: TestUploadInput<'a>,
        ) -> BoxFuture<'a, Result<String, AssetError>> {
            Box::pin(async move {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                if active > self.peak.load(Ordering::SeqCst) {
                    self.peak.store(active, Ordering::SeqCst);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
                self.active.fetch_sub(1, Ordering::SeqCst);
                Ok("https://cdn.example/slow.png".to_string())
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrency_gate_caps_parallel_uploads_at_four() {
        let provider = Arc::new(SlowProvider {
            active: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        });
        let host = Arc::new(ImageHost::with_providers(
            vec![provider.clone()],
            Vec::new(),
            MockHttp::shared(Vec::new()),
        ));
        let mut handles = Vec::new();
        for index in 0..8u8 {
            let host = Arc::clone(&host);
            handles.push(tokio::spawn(async move {
                let bytes = png_bytes(u32::from(index) + 1, 1);
                host.resolve(ImageSource::Bytes(&bytes), ctx()).await
            }));
        }
        for handle in handles {
            let resolved = handle.await.unwrap().unwrap();
            assert_eq!(resolved.provider, "slow");
        }
        assert!(provider.peak.load(Ordering::SeqCst) <= MAX_CONCURRENT_IMAGES as u64);
        assert_eq!(host.stats().uploaded, 8);
    }

    #[test]
    fn is_local_matches_go_rules() {
        assert!(is_local("data:image/png;base64,AAAA"));
        assert!(is_local("file:///tmp/a.png"));
        assert!(is_local("/tmp/a.png"));
        assert!(is_local("./a.png"));
        assert!(is_local("http://localhost/a.png"));
        assert!(is_local("http://LOCALHOST:8080/a.png"));
        assert!(is_local("http://nas.local/a.png"));
        assert!(is_local("http://127.0.0.1/a.png"));
        assert!(is_local("http://[::1]/a.png"));
        assert!(is_local("http://10.0.0.5/a.png"));
        assert!(is_local("http://172.16.3.4/a.png"));
        assert!(is_local("http://192.168.1.1/a.png"));
        assert!(is_local("http://0.0.0.0/a.png"));
        assert!(is_local("http://user:pw@10.1.2.3:8080/a.png"));
        assert!(!is_local("https://cdn.example.com/a.png"));
        assert!(!is_local("http://8.8.8.8/a.png"));
        assert!(!is_local("http://172.32.0.1/a.png"));
    }

    fn breaker_host(
        providers: Vec<Arc<TestProvider>>,
        clock: &Arc<crate::testkit::FakeClock>,
    ) -> ImageHost {
        let providers: Vec<Arc<dyn Provider>> = providers
            .into_iter()
            .map(|provider| provider as Arc<dyn Provider>)
            .collect();
        ImageHost::with_providers(providers, Vec::new(), MockHttp::shared(Vec::new())).with_breaker(
            Arc::new(CircuitBreaker::with_clock(
                BreakerConfig::new(3, Duration::from_secs(60), Duration::from_secs(120)),
                clock.clone(),
            )),
        )
    }

    #[tokio::test]
    async fn breaker_stops_retrying_a_failing_provider_after_three_failures() {
        let clock = crate::testkit::FakeClock::shared();
        let flaky = Arc::new(TestProvider::always_failing("flaky", "boom"));
        let good = Arc::new(TestProvider::new("good"));
        let host = breaker_host(vec![flaky.clone(), good.clone()], &clock);
        for round in 0..4 {
            let bytes = png_bytes(2 + round, 2);
            let resolved = host
                .resolve(ImageSource::Bytes(&bytes), ctx())
                .await
                .expect("应当有可用图床");
            assert_eq!(resolved.provider, "good");
        }
        assert_eq!(flaky.calls(), 3);
        assert_eq!(good.calls(), 4);
        assert!(host.breaker().is_open("flaky"));
    }

    #[tokio::test]
    async fn all_open_breakers_fall_back_to_the_highest_priority_provider() {
        let clock = crate::testkit::FakeClock::shared();
        let first = Arc::new(TestProvider::always_failing("first", "boom"));
        let second = Arc::new(TestProvider::always_failing("second", "bang"));
        let host = breaker_host(vec![first.clone(), second.clone()], &clock);
        for round in 0..3 {
            let bytes = png_bytes(2 + round, 2);
            let err = host
                .resolve(ImageSource::Bytes(&bytes), ctx())
                .await
                .unwrap_err();
            assert!(err.to_string().starts_with("all providers failed: "));
        }
        assert_eq!(first.calls(), 3);
        assert_eq!(second.calls(), 3);
        let bytes = png_bytes(9, 9);
        let err = host
            .resolve(ImageSource::Bytes(&bytes), ctx())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("[first] boom"));
        assert_eq!(first.calls(), 4);
        assert_eq!(second.calls(), 3);
    }

    #[tokio::test]
    async fn probe_after_cooldown_restores_the_provider() {
        let clock = crate::testkit::FakeClock::shared();
        let flaky = Arc::new(TestProvider::failing_times("flaky", "boom", 3));
        let good = Arc::new(TestProvider::new("good"));
        let host = breaker_host(vec![flaky.clone(), good.clone()], &clock);
        for round in 0..3 {
            let bytes = png_bytes(2 + round, 2);
            let resolved = host
                .resolve(ImageSource::Bytes(&bytes), ctx())
                .await
                .expect("降级图床应当成功");
            assert_eq!(resolved.provider, "good");
        }
        assert!(host.breaker().is_open("flaky"));
        clock.advance(Duration::from_secs(61));
        let bytes = png_bytes(8, 8);
        let resolved = host
            .resolve(ImageSource::Bytes(&bytes), ctx())
            .await
            .expect("半开探测应当成功");
        assert_eq!(resolved.provider, "flaky");
        assert!(!host.breaker().is_open("flaky"));
        assert_eq!(flaky.calls(), 4);
    }
}
