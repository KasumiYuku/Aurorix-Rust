use std::path::PathBuf;

use aurorix_admin::{router, AdminState, IMMUTABLE_CACHE};
use aurorix_db::AppConfig;
use axum::body::Bytes;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use tower::ServiceExt;

const PASSWORD: &str = "webui-secret";

fn asset_uri(name_part: &str, extension: &str) -> String {
    let asset = aurorix_admin::webui::EMBEDDED_ASSETS
        .iter()
        .find(|asset| asset.name.contains(name_part) && asset.name.ends_with(extension))
        .expect("前端产物里要能找到对应文件");
    format!("/admin/{}", asset.name)
}

fn index_js() -> String {
    asset_uri("index", ".js")
}

fn index_css() -> String {
    asset_uri("index", ".css")
}

fn messages_js() -> String {
    asset_uri("messages", ".js")
}

fn script_tag() -> String {
    format!(r#"src="{}""#, index_js())
}

fn fixture(name: &str) -> AdminState {
    let dir = std::env::temp_dir().join(format!("aurorix-webui-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path: PathBuf = dir.join("config.json");
    let raw =
        format!(r#"{{"port":8080,"appid":"app-1","secret":"s3","admin_password":"{PASSWORD}"}}"#);
    std::fs::write(&path, raw).expect("写配置文件");
    let config = AppConfig::load(&path).expect("读配置文件");
    AdminState::new(&path, config)
}

fn request(method: Method, uri: &str) -> Request<Bytes> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Bytes::new())
        .expect("构造请求")
}

async fn call(app: &Router, request: Request<Bytes>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = request.map(axum::body::Body::from);
    let response = app.clone().oneshot(request).await.expect("路由应可调用");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 23)
        .await
        .expect("读响应体");
    (status, headers, bytes.to_vec())
}

fn header_text(headers: &HeaderMap, name: axum::http::HeaderName) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn get(app: &Router, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    call(app, request(Method::GET, uri)).await
}

#[tokio::test]
async fn root_redirects_to_the_admin_prefix() {
    let app = router(fixture("root"));
    let (status, headers, _) = get(&app, "/").await;
    assert!(
        status.is_redirection(),
        "GET / 需重定向到管理台前缀, 实际 {status}"
    );
    assert_eq!(
        header_text(&headers, header::LOCATION),
        "/admin/",
        "重定向目标需是 /admin/"
    );
}

#[tokio::test]
async fn spa_deep_links_without_extension_serve_index_html() {
    let app = router(fixture("deep-links"));
    for uri in ["/admin/login", "/admin/overview", "/admin/logs"] {
        let (status, headers, bytes) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} 需返回 200");
        assert!(
            header_text(&headers, header::CONTENT_TYPE).starts_with("text/html"),
            "{uri} 需返回 html"
        );
        let text = String::from_utf8(bytes).expect("index.html 需为 UTF-8");
        assert!(text.contains(&script_tag()), "{uri} 需返回首页");
    }
}

#[tokio::test]
async fn admin_root_serves_embedded_index_html() {
    let app = router(fixture("admin-root"));
    for uri in ["/admin", "/admin/"] {
        let (status, headers, bytes) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} 需返回 200");
        assert!(
            header_text(&headers, header::CONTENT_TYPE).starts_with("text/html"),
            "{uri} 需返回 html"
        );
        let text = String::from_utf8(bytes).expect("index.html 需为 UTF-8");
        assert!(text.contains(&script_tag()), "{uri} 需返回首页");
    }
}

#[tokio::test]
async fn embedded_assets_are_served_with_immutable_cache() {
    let app = router(fixture("assets"));
    for (uri, kind) in [
        (index_js().as_str(), "javascript"),
        (index_css().as_str(), "text/css"),
        (messages_js().as_str(), "javascript"),
    ] {
        let (status, headers, bytes) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} 需返回 200");
        assert!(
            header_text(&headers, header::CONTENT_TYPE).contains(kind),
            "{uri} 的类型需为 {kind}: {}",
            header_text(&headers, header::CONTENT_TYPE)
        );
        assert_eq!(
            header_text(&headers, header::CACHE_CONTROL),
            IMMUTABLE_CACHE,
            "{uri} 需长缓存"
        );
        assert!(bytes.len() > 1024, "{uri} 内容不得为空");
    }
    let (status, _, bytes) = get(&app, &index_js().replace("/admin", "")).await;
    assert_eq!(status, StatusCode::OK, "不带 /admin 前缀也需可取到脚本");
    assert!(!bytes.is_empty(), "脚本内容不得为空");
}

#[tokio::test]
async fn spa_deep_link_falls_back_to_index_html() {
    let app = router(fixture("deep-link"));
    for uri in ["/admin/logs", "/admin/config", "/logs"] {
        let (status, headers, bytes) = get(&app, uri).await;
        assert_eq!(status, StatusCode::OK, "{uri} 需返回 200");
        assert!(
            header_text(&headers, header::CONTENT_TYPE).starts_with("text/html"),
            "{uri} 需回落首页"
        );
        let text = String::from_utf8(bytes).expect("index.html 需为 UTF-8");
        assert!(text.contains(&script_tag()), "{uri} 需返回首页内容");
    }
}

#[tokio::test]
async fn unknown_api_path_returns_json_404() {
    let app = router(fixture("api-404"));
    let (status, headers, bytes) = get(&app, "/admin/api/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "未知 API 需 404");
    assert_eq!(
        header_text(&headers, header::CONTENT_TYPE),
        "application/json",
        "未知 API 需 JSON"
    );
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("响应体需为 JSON");
    assert_eq!(
        value["error"],
        serde_json::json!("接口不存在"),
        "需返回中文错误"
    );
}

#[tokio::test]
async fn missing_asset_with_extension_returns_404() {
    let app = router(fixture("asset-404"));
    for uri in ["/favicon.ico", "/admin/assets/index-missing.js"] {
        let (status, _, _) = get(&app, uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} 不存在需 404");
    }
}
