use std::collections::BTreeMap;
use std::sync::Arc;

use aurorix_admin::{build_log_filter, router, AdminState, LogQuery};
use aurorix_db::AppConfig;
use aurorix_log::{Level, Logger, Options, RingFilter};
use axum::body::Bytes;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "sup3r-secret-pwd";

const APP_SECRET: &str = "app-s3cret";

fn logger_with_entries(name: &str) -> (Arc<Logger>, AdminState, Router) {
    let dir =
        std::env::temp_dir().join(format!("aurorix-admin-logs-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("config.json");
    let raw = format!(
        r#"{{"port":8080,"appid":"app-1","secret":"{APP_SECRET}","admin_password":"{PASSWORD}"}}"#
    );
    std::fs::write(&path, raw).expect("写配置文件");
    let config = AppConfig::load(&path).expect("读配置文件");
    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false),
    );
    logger.debug("imagegen", "生成图片 group=G1 user=U1");
    logger.info("message", "收到消息 group=G1 user=U2");
    logger.warn("db", "慢查询 group=G2 user=U1");
    logger.error("render", "渲染失败");
    let logs: Arc<dyn aurorix_admin::LogSource> = logger.clone();
    let state = AdminState::new(&path, config)
        .with_logs(logs)
        .with_logger(logger.clone());
    let app = router(state.clone());
    (logger, state, app)
}

fn get(uri: &str, token: &str) -> Request<Bytes> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Bytes::new())
        .expect("构造请求")
}

async fn call(app: &Router, request: Request<Bytes>) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request.map(axum::body::Body::from))
        .await
        .expect("路由应可调用");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("读响应体");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn login(app: &Router) -> String {
    let body = json!({ "admin_password": PASSWORD });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/admin/api/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Bytes::from(body.to_string()))
        .expect("构造请求");
    let response = app
        .clone()
        .oneshot(request.map(axum::body::Body::from))
        .await
        .expect("路由应可调用");
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("读响应体");
    serde_json::from_slice::<Value>(&bytes).expect("登录响应需为 JSON")["token"]
        .as_str()
        .expect("登录需返回 token")
        .to_string()
}

fn entry_count(body: &Value) -> usize {
    body["entries"].as_array().expect("entries 需为数组").len()
}

#[tokio::test]
async fn log_snapshot_keeps_the_go_json_shape() {
    let (_logger, _state, app) = logger_with_entries("shape");
    let token = login(&app).await;
    let (status, body) = call(&app, get("/admin/api/logs", &token)).await;
    assert_eq!(status, StatusCode::OK);
    for key in ["entries", "scopes", "total", "errors", "summary"] {
        assert!(body.get(key).is_some(), "响应需保留键 {key}");
    }
    assert_eq!(body["errors"], json!(1), "errors 需为 error 计数");
    assert_eq!(body["summary"]["levels"]["warn"], json!(1));
    let entries = body["entries"].as_array().expect("entries 需为数组");
    assert_eq!(
        Some(entries.len() as u64),
        body["total"].as_u64(),
        "未过滤时 entries 应覆盖全部记录"
    );
    assert!(entries.len() >= 4, "四条业务日志需都在");
    let scopes = body["scopes"].as_array().expect("scopes 需为数组");
    for expected in ["imagegen", "message", "db", "render"] {
        assert!(
            scopes.iter().any(|scope| scope.as_str() == Some(expected)),
            "scopes 需包含 {expected}"
        );
    }
    let ids: Vec<u64> = entries
        .iter()
        .map(|entry| entry["id"].as_u64().expect("id 需为数字"))
        .collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] > pair[1]),
        "记录需按编号倒序"
    );
    let messages: Vec<&str> = entries
        .iter()
        .filter_map(|entry| entry["msg"].as_str())
        .collect();
    for expected in [
        "生成图片 group=G1 user=U1",
        "收到消息 group=G1 user=U2",
        "慢查询 group=G2 user=U1",
        "渲染失败",
    ] {
        assert!(messages.contains(&expected), "缺少日志: {expected}");
    }
}

#[tokio::test]
async fn log_snapshot_filters_by_min_level() {
    let (_logger, _state, app) = logger_with_entries("level");
    let token = login(&app).await;
    let (status, body) = call(&app, get("/admin/api/logs?min_level=warn", &token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(entry_count(&body), 2, "warn 及以上两条");
    for entry in body["entries"].as_array().expect("数组") {
        let level = entry["level"].as_str().expect("级别");
        assert!(
            level == "WARN" || level == "ERROR",
            "级别需 >= warn: {level}"
        );
    }
    let (status, _) = call(&app, get("/admin/api/logs?min_level=bogus", &token)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "非法级别需 400");
}

#[tokio::test]
async fn log_snapshot_filters_by_module_and_legacy_scope() {
    let (_logger, _state, app) = logger_with_entries("module");
    let token = login(&app).await;
    let (_, by_module) = call(&app, get("/admin/api/logs?module=message", &token)).await;
    let (_, by_scope) = call(&app, get("/admin/api/logs?scope=message", &token)).await;
    assert_eq!(entry_count(&by_module), 1, "module 参数需生效");
    assert_eq!(entry_count(&by_scope), 1, "旧 scope 参数需继续支持");
    assert_eq!(by_module["entries"][0]["scope"], json!("message"));
}

#[tokio::test]
async fn log_snapshot_filters_by_keyword_group_user_and_trace() {
    let (logger, _state, app) = logger_with_entries("context");
    let token = login(&app).await;
    let (_, keyword) = call(&app, get("/admin/api/logs?q=G1", &token)).await;
    assert_eq!(entry_count(&keyword), 2, "q 需搜正文");
    let (_, keyword) = call(&app, get("/admin/api/logs?q=imagegen", &token)).await;
    assert_eq!(entry_count(&keyword), 1, "q 需搜模块名");
    let (_, scene) = call(&app, get("/admin/api/logs?group=G1&user=U2", &token)).await;
    assert_eq!(entry_count(&scene), 1, "group+user 组合");
    let (_, users) = call(&app, get("/admin/api/logs?user=U1", &token)).await;
    assert_eq!(entry_count(&users), 2, "user 单条件");
    let mut record =
        aurorix_log::LogRecord::new(Level::Info, "gateway", "带 trace", BTreeMap::new());
    record.trace = Some("m3f9a1-17".to_string());
    logger.ring().push(record);
    let (_, traced) = call(&app, get("/admin/api/logs?trace=m3f9a1-17", &token)).await;
    assert_eq!(entry_count(&traced), 1, "trace 过滤");
}

#[tokio::test]
async fn log_snapshot_applies_limit() {
    let (_logger, _state, app) = logger_with_entries("limit");
    let token = login(&app).await;
    let (_, body) = call(&app, get("/admin/api/logs?limit=2", &token)).await;
    assert_eq!(entry_count(&body), 2, "limit 需生效");
    let (_, body) = call(&app, get("/admin/api/logs?limit=0", &token)).await;
    assert_eq!(
        Some(entry_count(&body) as u64),
        body["total"].as_u64(),
        "非法 limit 回落到默认值"
    );
}

#[test]
fn build_log_filter_maps_every_parameter() {
    let query = LogQuery {
        limit: Some(7),
        min_level: Some("warning".to_string()),
        module: Some("imagegen".to_string()),
        scope: Some("legacy".to_string()),
        q: Some("url".to_string()),
        group: Some("G1".to_string()),
        user: Some("U1".to_string()),
        trace: Some("t-1".to_string()),
    };
    let filter = build_log_filter(&query).expect("参数合法");
    assert_eq!(filter.limit, 7);
    assert_eq!(filter.min_level, Some(Level::Warn));
    assert_eq!(filter.module.as_deref(), Some("imagegen"), "module 优先");
    assert_eq!(filter.keyword.as_deref(), Some("url"));
    assert_eq!(filter.group.as_deref(), Some("G1"));
    assert_eq!(filter.user.as_deref(), Some("U1"));
    assert_eq!(filter.trace.as_deref(), Some("t-1"));
}

#[test]
fn build_log_filter_prefers_scope_when_module_is_absent() {
    let query = LogQuery {
        scope: Some("gateway".to_string()),
        ..LogQuery::default()
    };
    let filter = build_log_filter(&query).expect("参数合法");
    assert_eq!(filter.module.as_deref(), Some("gateway"));
    assert_eq!(filter.limit, aurorix_admin::DEFAULT_LOG_LIMIT);
    assert_eq!(
        filter,
        RingFilter::new()
            .with_limit(aurorix_admin::DEFAULT_LOG_LIMIT)
            .with_module("gateway")
    );
}

#[test]
fn build_log_filter_rejects_unknown_level() {
    let query = LogQuery {
        min_level: Some("trace".to_string()),
        ..LogQuery::default()
    };
    assert!(build_log_filter(&query).is_err(), "未知级别需报错");
}

#[test]
fn build_log_filter_ignores_blank_values() {
    let query = LogQuery {
        min_level: Some("   ".to_string()),
        module: Some(String::new()),
        q: Some("  ".to_string()),
        ..LogQuery::default()
    };
    let filter = build_log_filter(&query).expect("空白值不算错误");
    assert_eq!(
        filter,
        RingFilter::new().with_limit(aurorix_admin::DEFAULT_LOG_LIMIT)
    );
}
