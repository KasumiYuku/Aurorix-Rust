pub mod breaker;
pub mod cache;
pub mod config;
pub mod error;
pub mod host;
pub mod http;
pub mod image_size;
pub mod input;
pub mod markdown;
pub mod mime;
pub mod official;
pub mod provider;
pub mod providers;
pub mod registry;
pub mod sign;

#[cfg(test)]
pub(crate) mod testkit;

pub use breaker::{
    BreakerClock, BreakerConfig, BreakerEvent, BreakerSnapshot, BreakerVerdict, CircuitBreaker,
    DEFAULT_COOLDOWN_MAX, DEFAULT_COOLDOWN_MIN, DEFAULT_FAIL_THRESHOLD,
};
pub use cache::{
    set_url_cache_policy, set_url_cache_probe_percent, set_url_cache_renewals,
    set_url_cache_seconds, set_url_cache_verify, url_cache_policy, CacheLookup, DedupEntry,
    UploadDedupCache, UrlCachePolicy, DEFAULT_URL_CACHE_POLICY, DEFAULT_URL_CACHE_PROBE_PERCENT,
    DEFAULT_URL_CACHE_RENEWALS, DEFAULT_URL_CACHE_SECONDS, MAX_URL_CACHE_PROBE_PERCENT,
    MIN_URL_CACHE_PROBE_PERCENT,
};
pub use config::{HostConfig, ProviderItem};
pub use error::{AssetError, AssetResult};
pub use host::{is_local, HostStats, ImageHost, ImageSource, MAX_CONCURRENT_IMAGES};
pub use http::{
    BoxFuture, HttpClient, HttpMethod, HttpRequest, HttpResponse, Multipart, ReqwestClient,
};
pub use image_size::{
    probe, probe_dimensions, CHROME_USER_AGENT, DIMENSION_TIMEOUT_SECS, MAX_PROBE_BYTES,
};
pub use input::{decode, decode_image, MediaInput, MediaSource};
pub use official::{official_upload, set_official_upload, OfficialUploadFn, OfficialUploadRequest};
pub use provider::{MediaKind, MediaKinds, Provider, ResolvedImage, UploadContext, UploadInput};
pub use registry::{ConfigField, ProviderEntry, ProviderFactory, ProviderRegistry, ProviderSpec};
pub use sign::{bkn, cos_sign, urlencode_go_compat};

pub const CRATE_NAME: &str = "aurorix-assets";
