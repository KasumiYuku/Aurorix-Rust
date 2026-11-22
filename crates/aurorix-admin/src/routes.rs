use std::collections::BTreeMap;

use aurorix_db::StatsView;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::sse::Sse;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use aurorix_log::{Level, RingFilter};

use crate::assets;
use crate::auth::{password_matches, session_token, SESSION_COOKIE};
use crate::bans;
use crate::config::{
    config_view, is_secret, is_secret_placeholder, is_writable, masked_values, needs_restart,
};
use crate::error::{invalid_body, not_found, ApiError};
use crate::inbox;
use crate::jobs;
use crate::live::LiveStream;
use crate::menu;
use crate::profile;
use crate::sources::{PluginError, PluginInfo};
use crate::state::AdminState;
use crate::system;
use crate::webui;

pub const MAX_LOG_LIMIT: usize = 2048;

pub const DEFAULT_LOG_LIMIT: usize = 500;

pub const MODULE: &str = "admin";

pub const REMEMBER_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

pub fn router(state: AdminState) -> Router {
    let protected = Router::new()
        .route("/admin/api/me", get(handle_me))
        .route("/admin/api/overview", get(handle_status))
        .route("/admin/api/status", get(handle_status))
        .route("/admin/api/logs", get(handle_logs))
        .route("/admin/api/stream", get(handle_logs_stream))
        .route("/admin/api/logs/stream", get(handle_logs_stream))
        .route("/admin/api/stats", get(handle_stats))
        .route(
            "/admin/api/config",
            get(handle_get_config)
                .put(handle_post_config)
                .post(handle_post_config),
        )
        .route("/admin/api/plugins", get(handle_plugins))
        .route(
            "/admin/api/plugins/{id}",
            get(handle_plugin).put(handle_plugin_config),
        )
        .route("/admin/api/plugins/{id}/access", put(handle_plugin_access))
        .route("/admin/api/menu/meta", get(menu::meta))
        .route("/admin/api/menu/sync", post(menu::sync_panels))
        .route(
            "/admin/api/menu/panels",
            get(menu::list_panels).post(menu::create_panel),
        )
        .route(
            "/admin/api/menu/panels/{id}",
            put(menu::update_panel).delete(menu::delete_panel),
        )
        .route("/admin/api/menu/remote-sync", post(menu::sync_remote))
        .route("/admin/api/menu/snapshots", get(menu::list_snapshots))
        .route(
            "/admin/api/menu/snapshots/{file}/restore",
            post(menu::restore_snapshot),
        )
        .route("/admin/api/bans", get(bans::list).post(bans::put))
        .route("/admin/api/bans/{openid}", delete(bans::delete))
        .route("/admin/api/inbox/conversations", get(inbox::conversations))
        .route(
            "/admin/api/inbox/messages",
            get(inbox::messages).post(inbox::send),
        )
        .route("/admin/api/jobs", get(jobs::list))
        .route("/admin/api/jobs/{id}/pause", post(jobs::pause))
        .route("/admin/api/assets", get(assets::get).put(assets::put))
        .route("/admin/api/profile", get(profile::get))
        .route("/admin/api/profile/refresh", post(profile::refresh))
        .route("/admin/api/system/restart", post(system::restart))
        .route("/admin/api/system/stop", post(system::stop))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_auth,
        ));
    let app = Router::new()
        .route("/admin/api/login", post(handle_login))
        .route("/admin/api/logout", post(handle_logout))
        .merge(protected)
        .fallback(handle_fallback)
        .with_state(state.clone());
    mount_web_uis(app, &state)
}

fn mount_web_uis(mut app: Router, state: &AdminState) -> Router {
    for ui in state.webui.web_uis() {
        let id = ui.id.trim_matches('/').to_string();
        if id.is_empty() {
            continue;
        }
        if let Some(api) = ui.api {
            let nested = api.fallback(handle_plugin_api_not_found).layer(
                axum::middleware::from_fn_with_state(state.clone(), require_auth),
            );
            app = app.nest_service(&format!("/{id}/api"), nested);
        }
        app = match ui.spa_root {
            Some(root) => webui::mount_spa(app, &id, root),
            None => webui::mount_missing(app, &id),
        };
    }
    app
}

async fn handle_plugin_api_not_found() -> ApiError {
    not_found()
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    #[serde(default)]
    password: String,
    #[serde(default)]
    admin_password: String,
    #[serde(default)]
    remember: bool,
}

impl LoginRequest {
    fn secret(&self) -> &str {
        if self.password.is_empty() {
            &self.admin_password
        } else {
            &self.password
        }
    }
}

fn session_cookie(token: &str, max_age: u64) -> Result<HeaderValue, ApiError> {
    let value =
        format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
    HeaderValue::from_str(&value)
        .map_err(|err| ApiError::Server(format!("会话 Cookie 生成失败: {err}")))
}

async fn handle_login(
    State(state): State<AdminState>,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    let expected = state.password();
    if expected.is_empty() {
        return Err(ApiError::Unavailable(
            "远程管理未启用，请在 config.json 中设置 admin_password".to_string(),
        ));
    }
    if !password_matches(&expected, input.secret()) {
        state.logger.warn(MODULE, "管理台登录失败");
        return Err(ApiError::Unauthorized("密码错误".to_string()));
    }
    let ttl = if input.remember {
        std::time::Duration::from_secs(REMEMBER_TTL_SECONDS)
    } else {
        state.sessions.ttl()
    };
    let (token, expires_in) = state.sessions.issue_for(ttl);
    state.logger.info(MODULE, "管理台登录成功");
    let body = json!({ "ok": true, "token": token, "expires_in": expires_in });
    let mut response = (StatusCode::OK, Json(body)).into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        session_cookie(&token, expires_in)?,
    );
    Ok(response)
}

async fn handle_logout(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = session_token(&headers) {
        state.sessions.revoke(&token);
    }
    let mut response = (StatusCode::OK, Json(json!({ "ok": true }))).into_response();
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, session_cookie("", 0)?);
    Ok(response)
}

async fn handle_me() -> Json<Value> {
    Json(json!({ "ok": true, "admin": true }))
}

pub(crate) async fn overview_snapshot(state: &AdminState) -> Value {
    let view = state.status.snapshot();
    let logs = state.logs.overview();
    let stats = state.stats.view_async().await;
    json!({
        "now": now_ms(),
        "runtime": {
            "uptime_sec": view.uptime_seconds,
            "protocol": view.protocol,
            "port": view.port,
            "version": view.version,
            "tasks": view.tasks,
            "mem": { "rss_bytes": rss_bytes() },
            "counters": {
                "recv": stats.total_recv,
                "sent": stats.total_sent,
                "button": stats.today.button,
            },
        },
        "counts": view.counts,
        "logs": { "total": logs.total, "errors": logs.errors },
        "stats": stats,
        "gateway": view.gateway,
        "control": { "supervised": state.control.supervised },
    })
}

async fn handle_status(State(state): State<AdminState>) -> Json<Value> {
    Json(overview_snapshot(&state).await)
}

fn rss_bytes() -> u64 {
    let Ok(raw) = std::fs::read_to_string("/proc/self/statm") else {
        return 0;
    };
    let pages = raw
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    pages * 4096
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

#[derive(Debug, Default, Deserialize)]
pub struct LogQuery {
    pub limit: Option<i64>,
    pub min_level: Option<String>,
    pub module: Option<String>,
    pub scope: Option<String>,
    pub q: Option<String>,
    pub group: Option<String>,
    pub user: Option<String>,
    pub trace: Option<String>,
}

pub fn clamp_limit(limit: Option<i64>) -> usize {
    match limit {
        Some(value) if value > 0 && (value as usize) <= MAX_LOG_LIMIT => value as usize,
        _ => DEFAULT_LOG_LIMIT,
    }
}

fn query_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

pub fn build_log_filter(query: &LogQuery) -> Result<RingFilter, ApiError> {
    let mut filter = RingFilter::new().with_limit(clamp_limit(query.limit));
    if let Some(text) = query_text(query.min_level.as_deref()) {
        let level = Level::parse(&text).map_err(|err| ApiError::BadRequest(err.to_string()))?;
        filter.min_level = Some(level);
    }
    filter.module =
        query_text(query.module.as_deref()).or_else(|| query_text(query.scope.as_deref()));
    filter.keyword = query_text(query.q.as_deref());
    filter.group = query_text(query.group.as_deref());
    filter.user = query_text(query.user.as_deref());
    filter.trace = query_text(query.trace.as_deref());
    Ok(filter)
}

async fn handle_logs(
    State(state): State<AdminState>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = build_log_filter(&query)?;
    let entries = state.logs.query(&filter);
    let overview = state.logs.overview();
    let scopes: Vec<String> = overview.scopes.keys().cloned().collect();
    Ok(Json(json!({
        "entries": entries,
        "scopes": scopes,
        "total": overview.total,
        "errors": overview.errors,
        "summary": { "levels": overview.levels, "scopes": overview.scopes },
    })))
}

async fn handle_logs_stream(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Sse<LiveStream> {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0);
    Sse::new(state.live.stream(last_event_id))
}

async fn handle_fallback(method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if path == "/admin/api" || path.starts_with("/admin/api/") {
        return not_found().into_response();
    }
    webui::handle_spa(method, uri).await
}

async fn handle_stats(State(state): State<AdminState>) -> Json<StatsView> {
    Json(state.stats.view_async().await)
}

async fn handle_plugins(State(state): State<AdminState>) -> Json<Vec<PluginInfo>> {
    Json(state.plugins.plugins())
}

async fn handle_plugin(
    State(state): State<AdminState>,
    Path(id): Path<String>,
) -> Result<Json<PluginInfo>, ApiError> {
    state
        .plugins
        .plugin(&id)
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("插件不存在".to_string()))
}

fn plugin_error(err: PluginError) -> ApiError {
    match err {
        PluginError::BadRequest(message) => ApiError::BadRequest(message),
        PluginError::NotFound(message) => ApiError::NotFound(message),
        PluginError::Server(_) => ApiError::Server("保存配置失败".to_string()),
    }
}

fn plugin_field_keys(plugin: &PluginInfo) -> Vec<String> {
    plugin
        .fields
        .iter()
        .filter_map(|field| field.get("key").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

fn validate_plugin_values(values: &Map<String, Value>, known: &[String]) -> Result<(), ApiError> {
    for key in values.keys() {
        if !known.iter().any(|item| item == key) {
            return Err(ApiError::BadRequest(format!("未知配置项: {key}")));
        }
    }
    Ok(())
}

async fn handle_plugin_config(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    payload: Result<Json<Map<String, Value>>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(values) = payload.map_err(|_| invalid_body())?;
    if let Some(plugin) = state.plugins.plugin(&id) {
        let known = plugin_field_keys(&plugin);
        if !known.is_empty() {
            validate_plugin_values(&values, &known)?;
        }
    }
    state
        .plugins
        .save_config(&id, values)
        .map_err(plugin_error)?;
    Ok(Json(json!({ "ok": true })))
}

async fn handle_plugin_access(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    payload: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(access) = payload.map_err(|_| invalid_body())?;
    state
        .plugins
        .save_access(&id, access)
        .map_err(|err| match err {
            PluginError::BadRequest(message) => ApiError::BadRequest(message),
            PluginError::NotFound(message) => ApiError::NotFound(message),
            PluginError::Server(_) => ApiError::Server("保存访问控制失败".to_string()),
        })?;
    Ok(Json(json!({ "ok": true })))
}

async fn handle_get_config(State(state): State<AdminState>) -> Json<Value> {
    let config = state.config_snapshot();
    Json(config_view(&config))
}

#[derive(Debug, Deserialize)]
struct ConfigRequest {
    #[serde(default)]
    values: BTreeMap<String, Value>,
}

async fn handle_post_config(
    State(state): State<AdminState>,
    payload: Result<Json<ConfigRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    let mut guard = state.lock_config();
    let mut next = guard.clone();
    let mut changed: Vec<String> = Vec::new();
    for (key, value) in &input.values {
        if !is_writable(key) {
            return Err(ApiError::BadRequest(format!("不支持修改的配置项: {key}")));
        }
        if value.is_null() {
            continue;
        }
        if is_secret(key) && is_secret_placeholder(value) {
            continue;
        }
        next.apply_core_key(key, value)
            .map_err(|err| ApiError::BadRequest(err.to_string()))?;
        changed.push(key.clone());
    }
    let mut restart_fields: Vec<String> = Vec::new();
    let mut hot_fields: Vec<String> = Vec::new();
    for key in &changed {
        if needs_restart(key) {
            restart_fields.push(key.clone());
        } else {
            hot_fields.push(key.clone());
        }
    }
    if !changed.is_empty() {
        let keys: Vec<&str> = changed.iter().map(String::as_str).collect();
        next.persist(&state.config_path, &keys)
            .map_err(|err| ApiError::Server(format!("保存配置失败: {err}")))?;
        *guard = next;
        drop(guard);
        if let Some(listener) = &state.config_listener {
            listener.applied(&state.config_snapshot());
        }
        state
            .logger
            .info(MODULE, &format!("配置已更新: {}", changed.join(", ")));
    } else {
        drop(guard);
    }
    let values = masked_values(&state.config_snapshot());
    let restart_needed = !restart_fields.is_empty();
    let body = json!({
        "ok": true,
        "values": values,
        "restart_required": restart_fields.clone(),
        "restart_needed": restart_needed,
        "restart_fields": restart_fields,
        "hot_fields": hot_fields,
    });
    Ok(Json(body))
}

async fn require_auth(
    State(state): State<AdminState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = session_token(request.headers())
        .ok_or_else(|| ApiError::Unauthorized("未登录或会话已失效".to_string()))?;
    if !state.sessions.validate(&token) {
        return Err(ApiError::Unauthorized("未登录或会话已失效".to_string()));
    }
    Ok(next.run(request).await)
}
