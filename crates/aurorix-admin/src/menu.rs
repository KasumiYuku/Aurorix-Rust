use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{invalid_body, ApiError};
use crate::remote::PanelSyncReport;
use crate::sources::{SourceError, SourceFuture};
use crate::state::AdminState;

pub const PANEL_NAME_MAX_WIDTH: usize = 14;

pub const PANEL_DESC_MAX_WIDTH: usize = 30;

pub const PANEL_REMARK_MAX_LEN: usize = 255;

pub const PANEL_MAX_ITEMS: usize = 20;

pub const PANEL_MAX_PER_BOT: usize = 20;

pub const PANEL_ITEM_COMMAND: &str = "command";

pub const PANEL_ITEM_LINK: &str = "link";

pub const PANEL_TARGET_ALL: &str = "all";

pub const PANEL_DISPLAY_NOTE: &str = "长度按显示宽度算: 中文与全角标点各计 2";

pub const PANEL_SCOPES: [(&str, &str); 4] = [
    ("group", "群聊"),
    ("c2c", "私聊"),
    ("channel", "频道"),
    ("dm", "频道私信"),
];

pub const DEFAULT_SNAPSHOT_DIR: &str = "data/menu-snapshots";

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelItem {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_admin: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub link: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panel {
    #[serde(default)]
    pub items: Vec<PanelItem>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remark: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub version: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelRecord {
    #[serde(default)]
    pub panel_id: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub target_type: String,
    #[serde(default)]
    pub panel: Panel,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub version: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_openids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_openids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuSnapshot {
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub panel_id: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub target_type: String,
    #[serde(default)]
    pub panel: Panel,
    #[serde(default)]
    pub version: i32,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub saved_at: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoredPanel {
    pub panel_id: String,
    pub scope: String,
    pub mode: String,
}

pub trait MenuSource: Send + Sync {
    fn panels(&self, _scope: &str) -> Result<Vec<PanelRecord>, String> {
        Ok(Vec::new())
    }

    fn count(&self) -> Result<usize, String> {
        Ok(0)
    }

    fn get(&self, _panel_id: &str) -> Option<PanelRecord> {
        None
    }

    fn create(&self, _scope: &str, _panel: Panel) -> Result<String, String> {
        Err("面板接口未启用".to_string())
    }

    fn update(&self, _panel_id: &str, _panel: Panel) -> Result<i32, String> {
        Err("面板不存在".to_string())
    }

    fn delete(&self, _panel_id: &str) -> Result<(), String> {
        Err("面板不存在".to_string())
    }

    fn snapshots(&self) -> Vec<MenuSnapshot> {
        Vec::new()
    }

    fn snapshot_dir(&self) -> String {
        DEFAULT_SNAPSHOT_DIR.to_string()
    }

    fn save_snapshot(&self, _action: &str, _record: &PanelRecord) -> Result<String, String> {
        Err("快照目录不可用".to_string())
    }

    fn read_snapshot(&self, _name: &str) -> Option<MenuSnapshot> {
        None
    }

    fn panels_async(
        &self,
        scope: String,
    ) -> SourceFuture<'_, Result<Vec<PanelRecord>, SourceError>> {
        Box::pin(async move { self.panels(&scope).map_err(SourceError::from_message) })
    }

    fn count_async(&self) -> SourceFuture<'_, Result<usize, SourceError>> {
        Box::pin(async move { self.count().map_err(SourceError::from_message) })
    }

    fn sync_async(&self) -> SourceFuture<'_, Result<PanelSyncReport, SourceError>> {
        Box::pin(async move { Ok(PanelSyncReport::default()) })
    }

    fn get_async(
        &self,
        panel_id: String,
    ) -> SourceFuture<'_, Result<Option<PanelRecord>, SourceError>> {
        Box::pin(async move { Ok(self.get(&panel_id)) })
    }

    fn create_async(
        &self,
        scope: String,
        panel: Panel,
    ) -> SourceFuture<'_, Result<String, SourceError>> {
        Box::pin(async move {
            self.create(&scope, panel)
                .map_err(SourceError::from_message)
        })
    }

    fn update_async(
        &self,
        panel_id: String,
        panel: Panel,
    ) -> SourceFuture<'_, Result<i32, SourceError>> {
        Box::pin(async move {
            self.update(&panel_id, panel)
                .map_err(SourceError::from_message)
        })
    }

    fn delete_async(&self, panel_id: String) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move { self.delete(&panel_id).map_err(SourceError::from_message) })
    }

    fn snapshots_async(&self) -> SourceFuture<'_, Result<Vec<MenuSnapshot>, SourceError>> {
        Box::pin(async move { Ok(self.snapshots()) })
    }

    fn save_snapshot_async(
        &self,
        action: String,
        record: PanelRecord,
    ) -> SourceFuture<'_, Result<String, SourceError>> {
        Box::pin(async move {
            self.save_snapshot(&action, &record)
                .map_err(SourceError::from_message)
        })
    }

    fn read_snapshot_async(
        &self,
        name: String,
    ) -> SourceFuture<'_, Result<Option<MenuSnapshot>, SourceError>> {
        Box::pin(async move { Ok(self.read_snapshot(&name)) })
    }

    fn restore_async(
        &self,
        name: String,
    ) -> SourceFuture<'_, Result<Option<RestoredPanel>, SourceError>> {
        Box::pin(async move {
            let Some(snapshot) = self.read_snapshot(&name) else {
                return Ok(None);
            };
            if self.get(&snapshot.panel_id).is_some() {
                self.update(&snapshot.panel_id, snapshot.panel.clone())
                    .map_err(SourceError::from_message)?;
                return Ok(Some(RestoredPanel {
                    panel_id: snapshot.panel_id,
                    scope: snapshot.scope,
                    mode: "overwrite".to_string(),
                }));
            }
            let created = self
                .create(&snapshot.scope, snapshot.panel.clone())
                .map_err(SourceError::from_message)?;
            Ok(Some(RestoredPanel {
                panel_id: created,
                scope: snapshot.scope,
                mode: "recreated".to_string(),
            }))
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyMenu;

impl MenuSource for EmptyMenu {}

pub fn valid_scope(scope: &str) -> bool {
    PANEL_SCOPES.iter().any(|(name, _)| *name == scope)
}

pub fn scope_label(scope: &str) -> &'static str {
    PANEL_SCOPES
        .iter()
        .find(|(name, _)| *name == scope)
        .map(|(_, label)| *label)
        .unwrap_or("")
}

pub fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| if (ch as u32) < 128 { 1 } else { 2 })
        .sum()
}

pub fn validate_panel(panel: &Panel) -> Result<(), String> {
    if panel.items.len() > PANEL_MAX_ITEMS {
        return Err(format!(
            "指令项 {} 条, 超过上限 {}",
            panel.items.len(),
            PANEL_MAX_ITEMS
        ));
    }
    let remark_len = panel.remark.chars().count();
    if remark_len > PANEL_REMARK_MAX_LEN {
        return Err(format!(
            "备注 {} 个字符, 超过上限 {}",
            remark_len, PANEL_REMARK_MAX_LEN
        ));
    }
    for (index, item) in panel.items.iter().enumerate() {
        let position = format!("第 {} 项", index + 1);
        if item.name.trim().is_empty() {
            return Err(format!("{position} 的名字不能为空"));
        }
        if item.desc.trim().is_empty() {
            return Err(format!("{position} 的描述不能为空"));
        }
        let width = display_width(&item.name);
        if width > PANEL_NAME_MAX_WIDTH {
            return Err(format!(
                "{position} 名字「{}」显示宽度 {width}, 超过上限 {PANEL_NAME_MAX_WIDTH}(中文与全角标点各计 2)",
                item.name
            ));
        }
        let width = display_width(&item.desc);
        if width > PANEL_DESC_MAX_WIDTH {
            return Err(format!(
                "{position} 描述「{}」显示宽度 {width}, 超过上限 {PANEL_DESC_MAX_WIDTH}(中文与全角标点各计 2)",
                item.desc
            ));
        }
        match item.kind.as_str() {
            PANEL_ITEM_COMMAND => {}
            PANEL_ITEM_LINK => {
                if !item.link.starts_with("https://") {
                    return Err(format!(
                        "{position} 是链接类型, link 必须以 https:// 开头(当前 \"{}\")",
                        item.link
                    ));
                }
            }
            other => {
                return Err(format!(
                    "{position} 的类型 \"{other}\" 不认识, 只能是 {PANEL_ITEM_COMMAND} 或 {PANEL_ITEM_LINK}"
                ))
            }
        }
    }
    Ok(())
}

pub fn editable(record: &PanelRecord) -> bool {
    record.target_type.is_empty() || record.target_type == PANEL_TARGET_ALL
}

fn panel_error(err: SourceError) -> ApiError {
    match err {
        SourceError::NotFound(message) => ApiError::NotFound(message),
        SourceError::BadRequest(message) => ApiError::BadRequest(message),
        SourceError::Conflict(message) => ApiError::Conflict(message),
        SourceError::Unavailable(message) | SourceError::Server(message) => {
            ApiError::BadGateway(message)
        }
    }
}

fn snapshot_notice(body: &mut serde_json::Map<String, Value>, snapshot: Result<String, String>) {
    match snapshot {
        Ok(file) => {
            body.insert("snapshot".to_string(), Value::String(file));
        }
        Err(err) => {
            body.insert("snapshot_error".to_string(), Value::String(err));
        }
    }
}

pub async fn meta() -> Json<Value> {
    let scopes: Vec<Value> = PANEL_SCOPES
        .iter()
        .map(|(scope, label)| json!({ "scope": scope, "label": label }))
        .collect();
    Json(json!({
        "scopes": scopes,
        "limits": {
            "name_width": PANEL_NAME_MAX_WIDTH,
            "desc_width": PANEL_DESC_MAX_WIDTH,
            "remark_len": PANEL_REMARK_MAX_LEN,
            "max_items": PANEL_MAX_ITEMS,
            "max_per_bot": PANEL_MAX_PER_BOT,
            "item_types": [PANEL_ITEM_COMMAND, PANEL_ITEM_LINK],
            "target_type": PANEL_TARGET_ALL,
            "display_note": PANEL_DISPLAY_NOTE,
        },
    }))
}

pub async fn list_panels(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    let mut scopes: Vec<Value> = Vec::with_capacity(PANEL_SCOPES.len());
    let mut total = 0usize;
    for (scope, label) in PANEL_SCOPES {
        let records = state
            .menu
            .panels_async(scope.to_string())
            .await
            .map_err(panel_error)?;
        total += records.len();
        scopes.push(json!({ "scope": scope, "label": label, "panels": records }));
    }
    Ok(Json(json!({
        "scopes": scopes,
        "total": total,
        "max_per_bot": PANEL_MAX_PER_BOT,
    })))
}

pub async fn sync_remote(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    let report = state.menu.sync_async().await.map_err(panel_error)?;
    Ok(Json(json!({
        "ok": report.failed == 0,
        "scopes": report.scopes,
        "created": report.created,
        "updated": report.updated,
        "deleted": report.deleted,
        "failed": report.failed,
        "errors": report.errors,
    })))
}

#[derive(Debug, Default, Deserialize)]
pub struct CreatePanelRequest {
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub panel: Panel,
}

pub async fn create_panel(
    State(state): State<AdminState>,
    payload: Result<Json<CreatePanelRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    if !valid_scope(&input.scope) {
        return Err(ApiError::BadRequest(format!(
            "场景 \"{}\" 不认识",
            input.scope
        )));
    }
    let count = state.menu.count_async().await.map_err(panel_error)?;
    if count >= PANEL_MAX_PER_BOT {
        return Err(ApiError::Conflict(format!(
            "该机器人已有 {count} 个面板, 达到上限 {PANEL_MAX_PER_BOT}(四个场景合计)。请先删掉不用的面板再新建"
        )));
    }
    let item_count = input.panel.items.len();
    let panel_id = state
        .menu
        .create_async(input.scope.clone(), input.panel)
        .await
        .map_err(panel_error)?;
    state.logger.info(
        crate::routes::MODULE,
        &format!(
            "新建指令面板: 场景={} id={panel_id} 条目={item_count}",
            input.scope
        ),
    );
    Ok(Json(json!({ "ok": true, "panel_id": panel_id })))
}

#[derive(Debug, Default, Deserialize)]
pub struct UpdatePanelRequest {
    #[serde(default)]
    pub panel: Panel,
}

pub async fn update_panel(
    State(state): State<AdminState>,
    Path(panel_id): Path<String>,
    payload: Result<Json<UpdatePanelRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    validate_panel(&input.panel).map_err(ApiError::BadRequest)?;
    let Some(current) = state
        .menu
        .get_async(panel_id.clone())
        .await
        .map_err(panel_error)?
    else {
        return Err(ApiError::NotFound("面板不存在".to_string()));
    };
    let snapshot = state
        .menu
        .save_snapshot_async("update".to_string(), current.clone())
        .await
        .map_err(|err| err.message().to_string());
    let version = state
        .menu
        .update_async(panel_id.clone(), input.panel.clone())
        .await
        .map_err(panel_error)?;
    state.logger.info(
        crate::routes::MODULE,
        &format!(
            "覆盖指令面板: id={panel_id} 条目 {} -> {} 版本={version}",
            current.panel.items.len(),
            input.panel.items.len()
        ),
    );
    let mut body = serde_json::Map::new();
    body.insert("ok".to_string(), Value::Bool(true));
    body.insert("version".to_string(), json!(version));
    snapshot_notice(&mut body, snapshot);
    Ok(Json(Value::Object(body)))
}

pub async fn delete_panel(
    State(state): State<AdminState>,
    Path(panel_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let Some(current) = state
        .menu
        .get_async(panel_id.clone())
        .await
        .map_err(panel_error)?
    else {
        return Err(ApiError::NotFound("面板不存在".to_string()));
    };
    let snapshot = state
        .menu
        .save_snapshot_async("delete".to_string(), current.clone())
        .await
        .map_err(|err| err.message().to_string());
    state
        .menu
        .delete_async(panel_id.clone())
        .await
        .map_err(panel_error)?;
    state.logger.warn(
        crate::routes::MODULE,
        &format!("删除指令面板: 场景={} id={panel_id}", current.scope),
    );
    let mut body = serde_json::Map::new();
    body.insert("ok".to_string(), Value::Bool(true));
    snapshot_notice(&mut body, snapshot);
    Ok(Json(Value::Object(body)))
}

#[derive(Debug, Default, Deserialize)]
pub struct SyncTarget {
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub panel_id: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct SyncRequest {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub targets: Vec<SyncTarget>,
}

#[derive(Debug, Default, Serialize)]
pub struct SyncResult {
    pub scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub panel_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mode: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub snapshot: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

pub async fn sync_panels(
    State(state): State<AdminState>,
    payload: Result<Json<SyncRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(input) = payload.map_err(|_| invalid_body())?;
    if input.from.trim().is_empty() {
        return Err(ApiError::BadRequest("没说要同步哪一份面板".to_string()));
    }
    let results = run_sync(&state, &input.from, &input.targets).await?;
    let done = results.iter().filter(|item| item.error.is_empty()).count();
    state.logger.info(
        crate::routes::MODULE,
        &format!(
            "同步指令面板: 源={} 目标={} 成功={} 失败={}",
            input.from,
            results.len(),
            done,
            results.len() - done
        ),
    );
    Ok(Json(json!({
        "ok": true,
        "from": input.from,
        "results": results,
    })))
}

async fn run_sync(
    state: &AdminState,
    from: &str,
    targets: &[SyncTarget],
) -> Result<Vec<SyncResult>, ApiError> {
    let Some(source) = state
        .menu
        .get_async(from.to_string())
        .await
        .map_err(panel_error)?
    else {
        return Err(ApiError::NotFound("面板不存在".to_string()));
    };
    if !editable(&source) {
        return Err(ApiError::BadRequest(
            "这份面板只对指定群或指定人生效, 不能作为同步的来源".to_string(),
        ));
    }
    if targets.is_empty() {
        return Err(ApiError::BadRequest("没有选要同步到哪个场景".to_string()));
    }
    let mut fresh = 0usize;
    for target in targets {
        if !valid_scope(&target.scope) {
            return Err(ApiError::BadRequest(format!(
                "场景 \"{}\" 不认识",
                target.scope
            )));
        }
        if target.panel_id.is_empty() {
            fresh += 1;
        }
    }
    if fresh > 0 {
        let count = state.menu.count_async().await.map_err(panel_error)?;
        if count + fresh > PANEL_MAX_PER_BOT {
            return Err(ApiError::BadRequest(format!(
                "要新建 {fresh} 份, 现在已有 {count} 份、上限 {PANEL_MAX_PER_BOT} —— 先删掉不用的面板"
            )));
        }
    }
    let content = Panel {
        items: source.panel.items.clone(),
        remark: source.panel.remark.clone(),
        version: 0,
    };
    let mut results = Vec::with_capacity(targets.len());
    for target in targets {
        results.push(sync_one(state, target, &content).await);
    }
    Ok(results)
}

async fn sync_one(state: &AdminState, target: &SyncTarget, content: &Panel) -> SyncResult {
    let mut result = SyncResult {
        scope: target.scope.clone(),
        panel_id: target.panel_id.clone(),
        ..SyncResult::default()
    };
    if target.panel_id.is_empty() {
        return match state
            .menu
            .create_async(target.scope.clone(), content.clone())
            .await
        {
            Ok(panel_id) => {
                result.panel_id = panel_id;
                result.mode = "created".to_string();
                result
            }
            Err(err) => {
                result.error = err.message().to_string();
                result
            }
        };
    }
    let current = match state.menu.get_async(target.panel_id.clone()).await {
        Ok(found) => found,
        Err(err) => {
            result.error = err.message().to_string();
            return result;
        }
    };
    let Some(current) = current else {
        result.error = "面板不存在".to_string();
        return result;
    };
    if !editable(&current) {
        result.error = "那份也只对指定群或指定人生效, 没动它".to_string();
        return result;
    }
    if current.scope != target.scope {
        result.error = "那份面板不在要同步的场景里".to_string();
        return result;
    }
    let snapshot = state
        .menu
        .save_snapshot_async("sync".to_string(), current.clone())
        .await;
    if let Err(err) = state
        .menu
        .update_async(target.panel_id.clone(), content.clone())
        .await
    {
        result.error = err.message().to_string();
        return result;
    }
    result.mode = "overwritten".to_string();
    if let Ok(file) = snapshot {
        result.snapshot = file;
    }
    result
}

pub async fn list_snapshots(State(state): State<AdminState>) -> Result<Json<Value>, ApiError> {
    let snapshots = state.menu.snapshots_async().await.map_err(panel_error)?;
    Ok(Json(json!({
        "snapshots": snapshots,
        "dir": state.menu.snapshot_dir(),
    })))
}

pub async fn restore_snapshot(
    State(state): State<AdminState>,
    Path(file): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if file.is_empty() || file.contains('/') || file.contains('\\') {
        return Err(ApiError::BadRequest("快照名不合法".to_string()));
    }
    let Some(restored) = state
        .menu
        .restore_async(file.clone())
        .await
        .map_err(panel_error)?
    else {
        return Err(ApiError::NotFound(
            "快照不存在或已损坏: 快照不存在".to_string(),
        ));
    };
    state.logger.warn(
        crate::routes::MODULE,
        &format!(
            "回滚指令面板: 快照={file} 场景={} id={} 模式={}",
            restored.scope, restored.panel_id, restored.mode
        ),
    );
    Ok(Json(json!({
        "ok": true,
        "panel_id": restored.panel_id,
        "scope": restored.scope,
        "mode": restored.mode,
    })))
}
