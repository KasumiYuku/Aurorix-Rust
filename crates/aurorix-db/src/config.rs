use std::collections::BTreeMap;
use std::path::Path;

use aurorix_core::constant;
use aurorix_core::RoleRequired;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{DbError, DbResult};

pub const DEFAULT_DATABASE: &str = "postgres://aurorix@127.0.0.1:5432/aurorix?sslmode=disable";

pub const DEFAULT_DATA_DIR: &str = "data";

pub const DEFAULT_PROXY: &str = "https://api.bot.qq.com";

pub const DEFAULT_LOG_LEVEL: &str = "info";

pub const DEFAULT_OVERRIDE_DIR: &str = "templates/override";

pub const DEFAULT_RELOAD_SECONDS: u64 = 3;

pub const MAX_IMAGE_SIDE: u32 = 4096;

pub const CONFIG_FILE: &str = "./config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Maintenance {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deny {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessRule {
    #[serde(default)]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub users: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_role"
    )]
    pub role: Option<RoleRequired>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance: Option<Maintenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny: Option<Deny>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessConfig {
    #[serde(default)]
    pub default: AccessRule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<BTreeMap<String, AccessRule>>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub maintenance: Maintenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny: Option<Deny>,
}

fn optional_role<'de, D>(deserializer: D) -> Result<Option<RoleRequired>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<Value>::deserialize(deserializer)?;
    match raw {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                return Ok(None);
            }
            text.parse::<RoleRequired>().map(Some).map_err(|_| {
                serde::de::Error::custom(format!("role 只能是 owner/admin/member, 收到 {text:?}"))
            })
        }
        Some(other) => Err(serde::de::Error::custom(format!(
            "role 需为字符串, 收到 {other}"
        ))),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplatesConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen_addr: Option<String>,
    #[serde(default)]
    pub appid: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub database: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub admin_password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intents: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_markdown: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_sync: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_buttons: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_buttons_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_buttons_group: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_buttons_c2c: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_buttons_channel: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_buttons_dm: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_font_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub welcome_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub welcome_messages: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farewell_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farewell_messages: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub welcome_extra: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub welcome_buttons: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farewell_extra: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farewell_buttons: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_when: Option<Vec<i32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_threshold: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_limits_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_max_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_max_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_pool: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefixes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit_mb: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gc_percent: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pprof_addr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_settings: Option<BTreeMap<String, BTreeMap<String, Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_access: Option<BTreeMap<String, AccessConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_maintenance: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_maintenance_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates: Option<TemplatesConfig>,
}

pub fn default_intents() -> Vec<String> {
    [
        "GROUP_AT_MESSAGE_CREATE",
        "GROUP_MESSAGE_CREATE",
        "C2C_MESSAGE_CREATE",
        "INTERACTION_CREATE",
        "GROUP_JOIN_REQUEST",
        "GROUP_MEMBER_ADD",
        "GROUP_MEMBER_REMOVE",
        "MESSAGE_AUDIT_PASS",
        "MESSAGE_AUDIT_REJECT",
        "GROUP_ADD_ROBOT",
        "GROUP_DEL_ROBOT",
        "GROUP_MSG_RECEIVE",
        "GROUP_MSG_REJECT",
    ]
    .iter()
    .map(|intent| intent.to_string())
    .collect()
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> DbResult<Self> {
        let raw = std::fs::read_to_string(path)?;
        let mut config: AppConfig = serde_json::from_str(&raw)?;
        config.normalize();
        Ok(config)
    }

    pub fn normalize(&mut self) {
        if self.database.is_empty() {
            self.database = DEFAULT_DATABASE.to_string();
        }
        if self.data_dir.as_deref().unwrap_or("").is_empty() {
            self.data_dir = Some(DEFAULT_DATA_DIR.to_string());
        }
        if self.protocol.as_deref().unwrap_or("").is_empty() {
            self.protocol = Some("webhook".to_string());
        }
        if self.listen_addr.as_deref().unwrap_or("").is_empty() {
            let default = if self.protocol() == "websocket" {
                "127.0.0.1"
            } else {
                "0.0.0.0"
            };
            self.listen_addr = Some(default.to_string());
        }
        self.gateway_url = self.gateway_url.take().map(|url| url.trim().to_string());
        self.keyboard_font_size = self
            .keyboard_font_size
            .take()
            .map(|size| size.trim().to_lowercase());
        if self.upload_threshold.unwrap_or(0) == 0 {
            self.upload_threshold = Some(constant::DEFAULT_UPLOAD_THRESHOLD as i64);
        }
        self.image_max_width = Some(clamp_image_side(
            self.image_max_width,
            constant::DEFAULT_IMAGE_MAX_WIDTH,
        ));
        self.image_max_height = Some(clamp_image_side(
            self.image_max_height,
            constant::DEFAULT_IMAGE_MAX_HEIGHT,
        ));
        if self.proxy.is_empty() {
            self.proxy = DEFAULT_PROXY.to_string();
        }
        if self.log_level.as_deref().unwrap_or("").is_empty() {
            self.log_level = Some(DEFAULT_LOG_LEVEL.to_string());
        }
        if self.intents.is_none() {
            self.intents = Some(default_intents());
        }
        if self.prefixes.is_none() {
            self.prefixes = Some(aurorix_core::prefix::default_prefix_chars());
        }
        self.pprof_addr = self.pprof_addr.take().map(|addr| normalize_pprof(&addr));
        let mut templates = self.templates.clone().unwrap_or_default();
        if templates.override_dir.as_deref().unwrap_or("").is_empty() {
            templates.override_dir = Some(DEFAULT_OVERRIDE_DIR.to_string());
        }
        if templates.reload_seconds.unwrap_or(0) == 0 {
            templates.reload_seconds = Some(DEFAULT_RELOAD_SECONDS);
        }
        self.templates = Some(templates);
    }

    pub fn database_url(&self) -> &str {
        if self.database.is_empty() {
            DEFAULT_DATABASE
        } else {
            &self.database
        }
    }

    pub fn protocol(&self) -> &str {
        self.protocol.as_deref().unwrap_or("webhook")
    }

    pub fn listen_addr(&self) -> &str {
        self.listen_addr.as_deref().unwrap_or("0.0.0.0")
    }

    pub fn data_dir(&self) -> &str {
        self.data_dir.as_deref().unwrap_or(DEFAULT_DATA_DIR)
    }

    pub fn log_level(&self) -> &str {
        self.log_level.as_deref().unwrap_or(DEFAULT_LOG_LEVEL)
    }

    pub fn upload_threshold(&self) -> i64 {
        self.upload_threshold
            .unwrap_or(constant::DEFAULT_UPLOAD_THRESHOLD as i64)
    }

    pub fn image_max_width(&self) -> u32 {
        self.image_max_width
            .unwrap_or(constant::DEFAULT_IMAGE_MAX_WIDTH)
    }

    pub fn image_max_height(&self) -> u32 {
        self.image_max_height
            .unwrap_or(constant::DEFAULT_IMAGE_MAX_HEIGHT)
    }

    pub fn intents(&self) -> Vec<String> {
        self.intents.clone().unwrap_or_else(default_intents)
    }

    pub fn prefixes(&self) -> Vec<String> {
        self.prefixes
            .clone()
            .unwrap_or_else(aurorix_core::prefix::default_prefix_chars)
    }

    pub fn retry_when(&self) -> Vec<i32> {
        self.retry_when.clone().unwrap_or_default()
    }

    pub fn templates(&self) -> TemplatesConfig {
        self.templates.clone().unwrap_or(TemplatesConfig {
            override_dir: Some(DEFAULT_OVERRIDE_DIR.to_string()),
            reload_seconds: Some(DEFAULT_RELOAD_SECONDS),
        })
    }

    pub fn maintenance_message(&self) -> &str {
        self.bot_maintenance_message
            .as_deref()
            .filter(|text| !text.is_empty())
            .unwrap_or(constant::DEFAULT_MAINTENANCE_MESSAGE)
    }

    pub fn plugin_settings_for(&self, plugin_id: &str) -> BTreeMap<String, Value> {
        self.plugin_settings
            .as_ref()
            .and_then(|settings| settings.get(plugin_id))
            .cloned()
            .unwrap_or_default()
    }

    pub fn plugin_access_for(&self, plugin_id: &str) -> AccessConfig {
        self.plugin_access
            .as_ref()
            .and_then(|access| access.get(plugin_id))
            .cloned()
            .unwrap_or_default()
    }

    pub fn footer_lines(&self, scene: aurorix_core::Scene) -> Vec<String> {
        let scoped = match scene {
            aurorix_core::Scene::Group => &self.footer_buttons_group,
            aurorix_core::Scene::C2c => &self.footer_buttons_c2c,
            aurorix_core::Scene::Channel => &self.footer_buttons_channel,
            aurorix_core::Scene::Dm => &self.footer_buttons_dm,
        };
        match scoped {
            Some(lines) if !lines.is_empty() => lines.clone(),
            _ => self.footer_buttons.clone().unwrap_or_default(),
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> DbResult<()> {
        let encoded = serde_json::to_string_pretty(self)?;
        std::fs::write(path, encoded + "\n")?;
        Ok(())
    }

    pub fn persist(&mut self, path: impl AsRef<Path>, keys: &[&str]) -> DbResult<()> {
        let path = path.as_ref().to_path_buf();
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        let mut root: BTreeMap<String, Value> = match raw.trim().is_empty() {
            true => BTreeMap::new(),
            false => serde_json::from_str(&raw)?,
        };
        let Value::Object(fields) = serde_json::to_value(&*self)? else {
            return Err(DbError::Config("配置不是对象".to_string()));
        };
        for key in keys {
            match fields.get(*key) {
                Some(value) => {
                    root.insert((*key).to_string(), value.clone());
                }
                None => {
                    root.remove(*key);
                }
            }
        }
        let updated = serde_json::to_string_pretty(&root)?;
        std::fs::write(&path, updated + "\n")?;
        self.normalize();
        Ok(())
    }

    pub fn save_plugin_settings(
        &mut self,
        path: impl AsRef<Path>,
        plugin_id: &str,
        settings: BTreeMap<String, Value>,
    ) -> DbResult<()> {
        let mut all = self.plugin_settings.clone().unwrap_or_default();
        all.insert(plugin_id.to_string(), settings);
        self.plugin_settings = Some(all);
        self.persist(path, &["plugin_settings"])
    }

    pub fn save_plugin_access(
        &mut self,
        path: impl AsRef<Path>,
        plugin_id: &str,
        access: AccessConfig,
    ) -> DbResult<()> {
        let mut all = self.plugin_access.clone().unwrap_or_default();
        all.insert(plugin_id.to_string(), access);
        self.plugin_access = Some(all);
        self.persist(path, &["plugin_access"])
    }

    pub fn apply_core_keys(&mut self, updates: &BTreeMap<String, Value>) -> DbResult<()> {
        for (key, value) in updates {
            self.apply_core_key(key, value)?;
        }
        Ok(())
    }

    pub fn apply_core_key(&mut self, key: &str, value: &Value) -> DbResult<()> {
        match key {
            "port" => {
                let port: u16 = typed(key, value)?;
                if port == 0 {
                    return Err(DbError::Config("port 不能为 0".to_string()));
                }
                self.port = port;
            }
            "listen_addr" => {
                let addr: String = typed(key, value)?;
                let addr = addr.trim().to_string();
                if !addr.is_empty() && addr.parse::<std::net::IpAddr>().is_err() {
                    return Err(DbError::Config(format!(
                        "listen_addr 必须是 IP 地址(不带端口): {addr}"
                    )));
                }
                self.listen_addr = Some(addr);
            }
            "appid" => self.appid = typed(key, value)?,
            "secret" => self.secret = typed(key, value)?,
            "proxy" => self.proxy = typed(key, value)?,
            "gateway_url" => {
                let url: String = typed(key, value)?;
                let url = url.trim().to_string();
                if !url.is_empty() && !url.starts_with("ws://") && !url.starts_with("wss://") {
                    return Err(DbError::Config(
                        "gateway_url 需以 ws:// 或 wss:// 开头".to_string(),
                    ));
                }
                self.gateway_url = Some(url);
            }
            "database" => {
                let url: String = typed(key, value)?;
                let url = url.trim().to_string();
                if !url.is_empty() && !is_postgres_url(&url) {
                    return Err(DbError::Config(
                        "database 需是 postgres:// 或 postgresql:// 开头的连接串".to_string(),
                    ));
                }
                self.database = url;
            }
            "data_dir" => {
                let dir: String = typed(key, value)?;
                let dir = dir.trim().to_string();
                self.data_dir = Some(if dir.is_empty() {
                    DEFAULT_DATA_DIR.to_string()
                } else {
                    dir
                });
            }
            "admin_password" => self.admin_password = typed(key, value)?,
            "protocol" => {
                let protocol: String = typed(key, value)?;
                if protocol != "webhook" && protocol != "websocket" {
                    return Err(DbError::Config(
                        "protocol 只能为 webhook 或 websocket".to_string(),
                    ));
                }
                self.protocol = Some(protocol);
            }
            "intents" => self.intents = Some(clean_strings(typed(key, value)?)),
            "global_markdown" => self.global_markdown = Some(typed(key, value)?),
            "retry_when" => self.retry_when = Some(typed(key, value)?),
            "upload_threshold" => {
                let threshold: i64 = typed(key, value)?;
                if threshold <= 0 {
                    return Err(DbError::Config("upload_threshold 必须大于 0".to_string()));
                }
                self.upload_threshold = Some(threshold);
            }
            "image_limits_enabled" => self.image_limits_enabled = Some(typed(key, value)?),
            "image_max_width" => self.image_max_width = Some(image_side(key, value)?),
            "image_max_height" => self.image_max_height = Some(image_side(key, value)?),
            "log_level" => {
                let level: String = typed(key, value)?;
                if !["debug", "info", "warn", "error"].contains(&level.as_str()) {
                    return Err(DbError::Config(
                        "log_level 只能为 debug/info/warn/error".to_string(),
                    ));
                }
                self.log_level = Some(level);
            }
            "footer_buttons" => self.footer_buttons = Some(clean_strings(typed(key, value)?)),
            "footer_buttons_group" => {
                self.footer_buttons_group = Some(clean_strings(typed(key, value)?))
            }
            "footer_buttons_c2c" => {
                self.footer_buttons_c2c = Some(clean_strings(typed(key, value)?))
            }
            "footer_buttons_channel" => {
                self.footer_buttons_channel = Some(clean_strings(typed(key, value)?))
            }
            "footer_buttons_dm" => self.footer_buttons_dm = Some(clean_strings(typed(key, value)?)),
            "keyboard_font_size" => {
                let size: String = typed(key, value)?;
                self.keyboard_font_size = Some(size.trim().to_lowercase());
            }
            "footer_buttons_enabled" => self.footer_buttons_enabled = Some(typed(key, value)?),
            "bot_maintenance" => self.bot_maintenance = Some(typed(key, value)?),
            "bot_maintenance_message" => {
                let message: String = typed(key, value)?;
                self.bot_maintenance_message = Some(message.trim().to_string());
            }
            "welcome_enabled" => self.welcome_enabled = Some(typed(key, value)?),
            "welcome_messages" => self.welcome_messages = Some(clean_strings(typed(key, value)?)),
            "farewell_enabled" => self.farewell_enabled = Some(typed(key, value)?),
            "farewell_messages" => self.farewell_messages = Some(clean_strings(typed(key, value)?)),
            "welcome_extra" => {
                let extra: String = typed(key, value)?;
                self.welcome_extra = Some(extra.trim().to_string());
            }
            "welcome_buttons" => self.welcome_buttons = Some(clean_strings(typed(key, value)?)),
            "farewell_extra" => {
                let extra: String = typed(key, value)?;
                self.farewell_extra = Some(extra.trim().to_string());
            }
            "farewell_buttons" => self.farewell_buttons = Some(clean_strings(typed(key, value)?)),
            "prefixes" => self.prefixes = Some(clean_strings(typed(key, value)?)),
            "memory_limit_mb" => self.memory_limit_mb = Some(typed(key, value)?),
            "gc_percent" => self.gc_percent = Some(typed(key, value)?),
            "pprof_addr" => {
                let addr: String = typed(key, value)?;
                self.pprof_addr = Some(normalize_pprof(&addr));
            }
            other => {
                return Err(DbError::Config(format!("未知配置项: {other}")));
            }
        }
        Ok(())
    }
}

fn normalize_pprof(addr: &str) -> String {
    match addr.strip_prefix(':') {
        Some(rest) => format!("127.0.0.1:{rest}"),
        None => addr.to_string(),
    }
}

fn clamp_image_side(value: Option<u32>, fallback: u32) -> u32 {
    match value {
        Some(side) if side > 0 && side <= MAX_IMAGE_SIDE => side,
        _ => fallback,
    }
}

fn image_side(key: &str, value: &Value) -> DbResult<u32> {
    let side: u32 = typed(key, value)?;
    if !(16..=MAX_IMAGE_SIDE).contains(&side) {
        return Err(DbError::Config(format!(
            "{key}: 取值需在 16~{MAX_IMAGE_SIDE} 之间, 收到 {side}"
        )));
    }
    Ok(side)
}

fn typed<T: DeserializeOwned>(key: &str, value: &Value) -> DbResult<T> {
    serde_json::from_value(value.clone()).map_err(|err| DbError::Config(format!("{key}: {err}")))
}

pub fn clean_strings(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

pub fn is_postgres_url(value: &str) -> bool {
    value.starts_with("postgres://") || value.starts_with("postgresql://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_config() -> AppConfig {
        AppConfig {
            appid: "app".to_string(),
            secret: "secret".to_string(),
            admin_password: "pwd".to_string(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn normalize_fills_go_defaults() {
        let mut config = AppConfig::default();
        config.normalize();
        assert_eq!(config.database_url(), DEFAULT_DATABASE);
        assert_eq!(config.data_dir(), DEFAULT_DATA_DIR);
        assert_eq!(config.protocol(), "webhook");
        assert_eq!(config.listen_addr(), "0.0.0.0");
        assert_eq!(config.proxy, DEFAULT_PROXY);
        assert_eq!(config.log_level(), "info");
        assert_eq!(config.upload_threshold(), 3_145_728);
        assert_eq!(config.image_max_width(), 350);
        assert_eq!(config.image_max_height(), 438);
        assert_eq!(config.intents().len(), 13);
        assert_eq!(
            config.prefixes(),
            vec!["/".to_string(), "#".to_string(), String::new()]
        );
        assert_eq!(
            config.templates().override_dir.as_deref(),
            Some(DEFAULT_OVERRIDE_DIR)
        );
        assert_eq!(
            config.maintenance_message(),
            constant::DEFAULT_MAINTENANCE_MESSAGE
        );
    }

    #[test]
    fn websocket_protocol_binds_loopback() {
        let mut config = AppConfig {
            protocol: Some("websocket".to_string()),
            ..AppConfig::default()
        };
        config.normalize();
        assert_eq!(config.listen_addr(), "127.0.0.1");
    }

    #[test]
    fn image_side_is_clamped() {
        let mut config = AppConfig {
            image_max_width: Some(0),
            image_max_height: Some(99_999),
            ..AppConfig::default()
        };
        config.normalize();
        assert_eq!(config.image_max_width(), 350);
        assert_eq!(config.image_max_height(), 438);
    }

    #[test]
    fn footer_lines_prefer_scene_then_fall_back() {
        let config = AppConfig {
            footer_buttons: Some(vec!["全局 | command | /help".to_string()]),
            footer_buttons_channel: Some(vec!["频道 | command | /c".to_string()]),
            ..AppConfig::default()
        };
        assert!(config.footer_lines(aurorix_core::Scene::Channel)[0].starts_with("频道"));
        assert!(config.footer_lines(aurorix_core::Scene::Group)[0].starts_with("全局"));
    }

    #[test]
    fn core_keys_validate_like_go() {
        let mut config = base_config();
        assert!(config.apply_core_key("port", &json!(0)).is_err());
        assert!(config.apply_core_key("protocol", &json!("grpc")).is_err());
        assert!(config.apply_core_key("log_level", &json!("trace")).is_err());
        assert!(config
            .apply_core_key("upload_threshold", &json!(0))
            .is_err());
        assert!(config.apply_core_key("image_max_width", &json!(0)).is_err());
        assert!(config.apply_core_key("image_max_width", &json!(15)).is_err());
        assert!(config
            .apply_core_key("image_max_height", &json!(4097))
            .is_err());
        assert!(config.apply_core_key("image_max_height", &json!(16)).is_ok());
        assert_eq!(config.image_max_height(), 16);
        assert!(config.apply_core_key("image_max_width", &json!(4096)).is_ok());
        assert_eq!(config.image_max_width(), 4096);
        assert!(config
            .apply_core_key("listen_addr", &json!("localhost"))
            .is_err());
        assert!(config
            .apply_core_key("database", &json!("mysql://x"))
            .is_err());
        assert!(config
            .apply_core_key("gateway_url", &json!("http://x"))
            .is_err());
        assert!(config.apply_core_key("unknown_key", &json!(1)).is_err());
        assert!(config.apply_core_key("port", &json!(8080)).is_ok());
        assert_eq!(config.port, 8080);
        assert!(config
            .apply_core_key("protocol", &json!("websocket"))
            .is_ok());
        assert!(config
            .apply_core_key("database", &json!("postgres://u@h:5432/db"))
            .is_ok());
        assert!(config
            .apply_core_key("gateway_url", &json!("wss://api.example.com"))
            .is_ok());
        assert!(config
            .apply_core_key("footer_buttons", &json!([" 帮助 | command | /help ", " "]))
            .is_ok());
        assert_eq!(config.footer_buttons.as_ref().map(Vec::len), Some(1));
        assert!(config.apply_core_key("pprof_addr", &json!(":6060")).is_ok());
        assert_eq!(config.pprof_addr.as_deref(), Some("127.0.0.1:6060"));
    }

    #[test]
    fn persist_only_touches_named_keys() {
        let dir = std::env::temp_dir().join(format!("aurorix-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{"port":8080,"appid":"a","secret":"s","admin_password":"p","custom_unknown":{"keep":1}}"#,
        )
        .expect("写配置");
        let mut config = AppConfig::load(&path).expect("读配置");
        assert!(config.apply_core_key("port", &json!(9000)).is_ok());
        config.persist(&path, &["port"]).expect("落盘");
        let raw = std::fs::read_to_string(&path).expect("读回");
        let value: Value = serde_json::from_str(&raw).expect("解析");
        assert_eq!(value["port"], json!(9000));
        assert_eq!(value["custom_unknown"]["keep"], json!(1));
        assert_eq!(value["appid"], json!("a"));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aurorix-cfg-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    #[test]
    fn saved_switches_survive_reload() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{"port":8080,"appid":"a","secret":"s","admin_password":"p"}"#,
        )
        .expect("写配置");
        let mut config = AppConfig::load(&path).expect("读配置");
        config
            .apply_core_key("bot_maintenance", &json!(true))
            .expect("开维护");
        config
            .apply_core_key("bot_maintenance_message", &json!("维护中, 稍后再来"))
            .expect("维护文案");
        config
            .persist(&path, &["bot_maintenance", "bot_maintenance_message"])
            .expect("落盘");
        let access = AccessConfig {
            maintenance: Maintenance {
                enabled: true,
                message: Some("插件维护中".to_string()),
            },
            ..AccessConfig::default()
        };
        config
            .save_plugin_access(&path, "rollpig", access)
            .expect("插件维护落盘");
        let reloaded = AppConfig::load(&path).expect("重新加载");
        assert_eq!(reloaded.bot_maintenance, Some(true));
        assert_eq!(
            reloaded.bot_maintenance_message.as_deref(),
            Some("维护中, 稍后再来")
        );
        assert_eq!(reloaded.maintenance_message(), "维护中, 稍后再来");
        let stored = reloaded.plugin_access_for("rollpig");
        assert!(stored.maintenance.enabled);
        assert_eq!(stored.maintenance.message.as_deref(), Some("插件维护中"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn persist_creates_a_missing_config_file() {
        let dir = scratch_dir("missing");
        let path = dir.join("config.json");
        let mut config = base_config();
        config
            .apply_core_key("bot_maintenance", &json!(true))
            .expect("开维护");
        config.persist(&path, &["bot_maintenance"]).expect("落盘");
        let reloaded = AppConfig::load(&path).expect("重新加载");
        assert_eq!(reloaded.bot_maintenance, Some(true));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_role_from_the_console_means_unrestricted() {
        let raw = r#"{"default":{"mode":"","users":[],"groups":[],"role":"","maintenance":{"enabled":false,"message":""},"deny":{}},"commands":{"/uid":{"mode":"whitelist","users":["u1"],"groups":[],"role":"admin"}},"disabled":false,"maintenance":{"enabled":false}}"#;
        let access: AccessConfig = serde_json::from_str(raw).expect("解析");
        assert_eq!(access.default.role, None);
        let rule = access.commands.as_ref().unwrap().get("/uid").expect("规则");
        assert_eq!(rule.role, Some(RoleRequired::Admin));
        assert_eq!(access.default.users, Some(Vec::new()));
    }

    #[test]
    fn access_config_deserializes_plugin_access_shape() {
        let raw = r#"{
            "default": { "mode": "off" },
            "deny": { "reply": true, "message": "这个插件只给群友用" },
            "commands": { "/uid": { "mode": "blacklist", "users": ["u1"], "deny": { "reply": false } } },
            "disabled": false,
            "maintenance": { "enabled": true, "message": "维护中" }
        }"#;
        let access: AccessConfig = serde_json::from_str(raw).expect("解析");
        assert_eq!(access.default.mode, "off");
        assert_eq!(
            access.deny.as_ref().unwrap().message.as_deref(),
            Some("这个插件只给群友用")
        );
        let rule = access.commands.as_ref().unwrap().get("/uid").expect("规则");
        assert_eq!(rule.mode, "blacklist");
        assert_eq!(rule.users.as_deref(), Some(&["u1".to_string()][..]));
        assert!(access.maintenance.enabled);
    }

    #[test]
    fn empty_listen_addr_and_gateway_url_stay_allowed() {
        let mut config = base_config();
        assert!(
            config.apply_core_key("listen_addr", &json!("")).is_ok(),
            "空 listen_addr 应当允许"
        );
        assert_eq!(
            config.listen_addr,
            Some(String::new()),
            "空 listen_addr 应当原样落库"
        );
        assert!(
            config.apply_core_key("gateway_url", &json!("")).is_ok(),
            "空 gateway_url 应当允许"
        );
        assert_eq!(
            config.gateway_url,
            Some(String::new()),
            "空 gateway_url 应当原样落库"
        );
        assert!(
            config
                .apply_core_key("listen_addr", &json!("1.2.3.4"))
                .is_ok(),
            "合法 IPv4 应当通过"
        );
        assert_eq!(config.listen_addr.as_deref(), Some("1.2.3.4"));
        assert!(
            config
                .apply_core_key("listen_addr", &json!("  ::1  "))
                .is_ok(),
            "合法 IPv6 应当通过"
        );
        assert_eq!(config.listen_addr.as_deref(), Some("::1"));
        assert!(
            config
                .apply_core_key("gateway_url", &json!("ws://127.0.0.1:3001"))
                .is_ok(),
            "合法 ws 地址应当通过"
        );
        assert_eq!(config.gateway_url.as_deref(), Some("ws://127.0.0.1:3001"));
        assert!(
            config
                .apply_core_key("listen_addr", &json!("1.2.3.4:8080"))
                .is_err(),
            "带端口的 listen_addr 应当被拒绝"
        );
        assert_eq!(
            config.listen_addr.as_deref(),
            Some("::1"),
            "被拒绝的写入不该覆盖原值"
        );
    }
}
