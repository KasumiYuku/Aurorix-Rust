use aurorix_core::Scene;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;

pub const PANEL_NAME_MAX_WIDTH: usize = 14;
pub const PANEL_DESC_MAX_WIDTH: usize = 30;
pub const PANEL_REMARK_MAX_LEN: usize = 255;
pub const PANEL_MAX_ITEMS: usize = 20;
pub const PANEL_MAX_PER_BOT: usize = 20;
pub const PANEL_ITEM_COMMAND: &str = "command";
pub const PANEL_ITEM_LINK: &str = "link";
pub const PANEL_TARGET_ALL: &str = "all";
pub const PANEL_PAGE_LIMIT: usize = 50;
pub const PANEL_MAX_PAGES: usize = 40;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelItem {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(rename = "type", default)]
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
pub struct PanelPage {
    #[serde(default)]
    pub records: Vec<PanelRecord>,
    #[serde(default)]
    pub next_cursor: String,
    #[serde(default)]
    pub is_end: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

pub fn panel_display_width(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

pub fn validate_panel(panel: &Panel) -> Result<(), String> {
    if panel.items.len() > PANEL_MAX_ITEMS {
        return Err(format!(
            "指令项 {} 个, 超过上限 {}",
            panel.items.len(),
            PANEL_MAX_ITEMS
        ));
    }
    let remark_len = panel.remark.chars().count();
    if remark_len > PANEL_REMARK_MAX_LEN {
        return Err(format!(
            "备注长度 {remark_len}, 超过上限 {PANEL_REMARK_MAX_LEN}"
        ));
    }
    for (index, item) in panel.items.iter().enumerate() {
        let position = index + 1;
        let name = item.name.trim();
        if name.is_empty() {
            return Err(format!("第 {position} 项 名字不能为空"));
        }
        let desc = item.desc.trim();
        if desc.is_empty() {
            return Err(format!("第 {position} 项 描述不能为空"));
        }
        let name_width = panel_display_width(name);
        if name_width > PANEL_NAME_MAX_WIDTH {
            return Err(format!(
                "第 {position} 项 名字「{name}」显示宽度 {name_width}, 超过上限 {PANEL_NAME_MAX_WIDTH}(中文与全角标点各计 2)"
            ));
        }
        let desc_width = panel_display_width(desc);
        if desc_width > PANEL_DESC_MAX_WIDTH {
            return Err(format!(
                "第 {position} 项 描述「{desc}」显示宽度 {desc_width}, 超过上限 {PANEL_DESC_MAX_WIDTH}(中文与全角标点各计 2)"
            ));
        }
    }
    Ok(())
}

pub fn valid_panel_scope(scope: &str) -> bool {
    Scene::ALL.iter().any(|scene| scene.as_str() == scope)
}

fn scope_error(scope: &str) -> ApiError {
    ApiError::Config(format!(
        "指令面板范围「{scope}」不合法, 只支持 group/c2c/channel/dm"
    ))
}

fn data_of(value: &serde_json::Value) -> &serde_json::Value {
    match value.get("data") {
        Some(data) if data.is_object() => data,
        _ => value,
    }
}

fn decode<T>(raw: &[u8], empty: &str) -> Result<T, ApiError>
where
    T: DeserializeOwned,
{
    if raw.is_empty() {
        return Err(ApiError::Config(String::from(empty)));
    }
    let value: serde_json::Value = serde_json::from_slice(raw)?;
    Ok(serde_json::from_value(data_of(&value).clone())?)
}

fn panel_path(panel_id: &str) -> String {
    format!("/v2/panels/{panel_id}")
}

impl crate::BotApi {
    pub async fn panel_list(&self, scope: &str) -> Result<Vec<PanelRecord>, ApiError> {
        if !valid_panel_scope(scope) {
            return Err(scope_error(scope));
        }
        let mut records: Vec<PanelRecord> = Vec::new();
        let mut cursor = String::new();
        for _ in 0..PANEL_MAX_PAGES {
            let mut path = format!("/v2/panels?scope={scope}&limit={PANEL_PAGE_LIMIT}");
            if !cursor.is_empty() {
                path.push_str("&cursor=");
                path.push_str(&cursor);
            }
            let raw = self
                .do_bytes(reqwest::Method::GET, &self.endpoint(&path), None)
                .await?;
            let page: PanelPage = decode(&raw, "指令面板列表返回为空")?;
            records.extend(page.records);
            if page.is_end || page.next_cursor.is_empty() {
                return Ok(records);
            }
            cursor = page.next_cursor;
        }
        Ok(records)
    }

    pub async fn panel_count(&self) -> Result<usize, ApiError> {
        let mut total = 0usize;
        for scene in Scene::ALL {
            total += self.panel_list(scene.as_str()).await?.len();
        }
        Ok(total)
    }

    pub async fn panel_get(&self, panel_id: &str) -> Result<PanelRecord, ApiError> {
        if panel_id.is_empty() {
            return Err(ApiError::Config(String::from("指令面板 id 为空")));
        }
        let raw = self
            .do_bytes(
                reqwest::Method::GET,
                &self.endpoint(&panel_path(panel_id)),
                None,
            )
            .await?;
        decode(&raw, "指令面板详情返回为空")
    }

    pub async fn panel_create(&self, scope: &str, panel: &Panel) -> Result<String, ApiError> {
        if !valid_panel_scope(scope) {
            return Err(scope_error(scope));
        }
        validate_panel(panel).map_err(ApiError::Config)?;
        let body = serde_json::json!({
            "scope": scope,
            "target_type": PANEL_TARGET_ALL,
            "panel": panel,
        });
        let raw = self
            .do_bytes(
                reqwest::Method::POST,
                &self.endpoint("/v2/panels"),
                Some(&serde_json::to_vec(&body)?),
            )
            .await?;
        let value: serde_json::Value = decode(&raw, "指令面板创建返回为空")?;
        let panel_id = value
            .get("panel_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if panel_id.is_empty() {
            return Err(ApiError::Config(String::from(
                "指令面板创建返回缺少 panel_id",
            )));
        }
        Ok(panel_id.to_string())
    }

    pub async fn panel_update(&self, panel_id: &str, panel: &Panel) -> Result<i32, ApiError> {
        if panel_id.is_empty() {
            return Err(ApiError::Config(String::from("指令面板 id 为空")));
        }
        validate_panel(panel).map_err(ApiError::Config)?;
        let mut submit = panel.clone();
        submit.version = 0;
        let body = serde_json::json!({ "panel": submit });
        let raw = self
            .do_bytes(
                reqwest::Method::PUT,
                &self.endpoint(&panel_path(panel_id)),
                Some(&serde_json::to_vec(&body)?),
            )
            .await?;
        let value: serde_json::Value = decode(&raw, "指令面板更新返回为空")?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| ApiError::Config(String::from("指令面板更新返回缺少 version")))?;
        i32::try_from(version).map_err(|_| ApiError::Config(String::from("指令面板版本号超出范围")))
    }

    pub async fn panel_delete(&self, panel_id: &str) -> Result<(), ApiError> {
        if panel_id.is_empty() {
            return Err(ApiError::Config(String::from("指令面板 id 为空")));
        }
        self.do_bytes(
            reqwest::Method::DELETE,
            &self.endpoint(&panel_path(panel_id)),
            None,
        )
        .await
        .map(|_| ())
    }
}
