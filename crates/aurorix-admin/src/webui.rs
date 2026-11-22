use std::path::{Path, PathBuf};

use axum::http::{header, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::error::not_found;

pub const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";

pub const NO_CACHE: &str = "no-cache";

pub const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";

pub const ASSET_EXTENSIONS: [&str; 20] = [
    ".js",
    ".mjs",
    ".css",
    ".map",
    ".json",
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".svg",
    ".webp",
    ".avif",
    ".ico",
    ".woff",
    ".woff2",
    ".ttf",
    ".otf",
    ".txt",
    ".webmanifest",
    ".xml",
];

#[derive(Clone)]
pub struct PluginWebUi {
    pub id: String,
    pub api: Option<Router>,
    pub spa_root: Option<PathBuf>,
}

impl PluginWebUi {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            api: None,
            spa_root: None,
        }
    }

    pub fn with_api(mut self, api: Router) -> Self {
        self.api = Some(api);
        self
    }

    pub fn with_spa_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.spa_root = Some(root.into());
        self
    }
}

pub trait WebUiSource: Send + Sync {
    fn web_uis(&self) -> Vec<PluginWebUi> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyWebUis;

impl WebUiSource for EmptyWebUis {}

pub struct StaticWebUis {
    pub items: Vec<PluginWebUi>,
}

impl WebUiSource for StaticWebUis {
    fn web_uis(&self) -> Vec<PluginWebUi> {
        self.items.clone()
    }
}

static GLOBAL_WEBUIS: std::sync::RwLock<Vec<PluginWebUi>> = std::sync::RwLock::new(Vec::new());

pub fn set_plugin_web_uis(items: Vec<PluginWebUi>) {
    let mut target = GLOBAL_WEBUIS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *target = items;
}

pub fn plugin_web_uis() -> Vec<PluginWebUi> {
    GLOBAL_WEBUIS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalWebUis;

impl WebUiSource for GlobalWebUis {
    fn web_uis(&self) -> Vec<PluginWebUi> {
        plugin_web_uis()
    }
}

pub fn clean_path(raw: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

pub fn extension(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, ext)) => format!(".{}", ext.to_ascii_lowercase()),
        None => String::new(),
    }
}

pub fn is_asset(name: &str) -> bool {
    let ext = extension(name);
    ASSET_EXTENSIONS.iter().any(|item| *item == ext)
}

pub fn content_type(name: &str) -> &'static str {
    match extension(name).as_str() {
        ".html" => "text/html; charset=utf-8",
        ".js" | ".mjs" => "text/javascript; charset=utf-8",
        ".css" => "text/css; charset=utf-8",
        ".json" | ".map" | ".webmanifest" => "application/json",
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".svg" => "image/svg+xml",
        ".webp" => "image/webp",
        ".avif" => "image/avif",
        ".ico" => "image/x-icon",
        ".woff" => "font/woff",
        ".woff2" => "font/woff2",
        ".ttf" => "font/ttf",
        ".otf" => "font/otf",
        ".txt" => "text/plain; charset=utf-8",
        ".xml" => "application/xml",
        ".wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn with_headers(
    status: StatusCode,
    cache: &'static str,
    kind: &'static str,
    body: Vec<u8>,
) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            (header::CONTENT_TYPE, HeaderValue::from_static(kind)),
        ],
        body,
    )
        .into_response()
}

pub async fn serve(root: &Path, raw_path: &str) -> Response {
    let Some(mut name) = clean_path(raw_path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if name.is_empty() {
        name = "index.html".to_string();
    }
    if name != "index.html" {
        let file = root.join(&name);
        if let Ok(metadata) = tokio::fs::metadata(&file).await {
            if metadata.is_file() {
                if let Ok(bytes) = tokio::fs::read(&file).await {
                    return with_headers(
                        StatusCode::OK,
                        IMMUTABLE_CACHE,
                        content_type(&name),
                        bytes,
                    );
                }
            }
        }
        if is_asset(&name) {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    match tokio::fs::read(root.join("index.html")).await {
        Ok(bytes) => with_headers(StatusCode::OK, NO_CACHE, HTML_CONTENT_TYPE, bytes),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn missing() -> Response {
    (StatusCode::NOT_FOUND, "404 page not found").into_response()
}

async fn serve_relative(root: &Path, path: &str, prefix: &str) -> Response {
    let relative = path.strip_prefix(prefix).unwrap_or(path);
    serve(root, relative).await
}

pub fn mount_spa(app: Router, id: &str, root: PathBuf) -> Router {
    let prefix = format!("/{id}");
    let index_root = root.clone();
    let index_prefix = prefix.clone();
    let index = move |uri: Uri| {
        let root = index_root.clone();
        let prefix = index_prefix.clone();
        let path = uri.path().to_string();
        async move { serve_relative(&root, &path, &prefix).await }
    };
    let path_root = root;
    let path_prefix = prefix;
    let path = move |uri: Uri| {
        let root = path_root.clone();
        let prefix = path_prefix.clone();
        let path = uri.path().to_string();
        async move { serve_relative(&root, &path, &prefix).await }
    };
    app.route(&format!("/{id}/"), get(index))
        .route(&format!("/{id}/{{*path}}"), get(path))
}

pub fn mount_missing(app: Router, id: &str) -> Router {
    app.route(&format!("/{id}/"), get(missing))
        .route(&format!("/{id}/{{*path}}"), get(missing))
}

pub const DIST_INDEX_NAME: &str = "index.html";

pub const DIST_ADMIN_PREFIX: &str = "admin/";

pub const DIST_API_PREFIX: &str = "api";

#[derive(Debug, Clone, Copy)]
pub struct EmbeddedAsset {
    pub name: &'static str,
    pub body: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/dist_manifest.rs"));

pub fn embedded_asset(name: &str) -> Option<&'static EmbeddedAsset> {
    EMBEDDED_ASSETS.iter().find(|asset| asset.name == name)
}

pub fn spa_path(raw: &str) -> Option<String> {
    let cleaned = clean_path(raw)?;
    if cleaned.is_empty() || cleaned == "admin" {
        return Some(String::new());
    }
    if let Some(rest) = cleaned.strip_prefix(DIST_ADMIN_PREFIX) {
        return Some(rest.to_string());
    }
    Some(cleaned)
}

pub fn is_api_path(name: &str) -> bool {
    name == DIST_API_PREFIX || name.starts_with("api/")
}

pub fn index_page() -> Response {
    with_headers(
        StatusCode::OK,
        NO_CACHE,
        HTML_CONTENT_TYPE,
        INDEX_HTML.as_bytes().to_vec(),
    )
}

pub fn serve_embedded(raw_path: &str) -> Response {
    let Some(name) = spa_path(raw_path) else {
        return not_found().into_response();
    };
    if name.is_empty() || name == DIST_INDEX_NAME {
        return index_page();
    }
    if let Some(asset) = embedded_asset(&name) {
        return with_headers(
            StatusCode::OK,
            IMMUTABLE_CACHE,
            content_type(asset.name),
            asset.body.as_bytes().to_vec(),
        );
    }
    if is_api_path(&name) || is_asset(&name) {
        return not_found().into_response();
    }
    index_page()
}

pub async fn handle_spa(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return not_found().into_response();
    }
    if uri.path() == "/" {
        return axum::response::Redirect::temporary("/admin/").into_response();
    }
    serve_embedded(uri.path())
}
