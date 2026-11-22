use std::collections::BTreeMap;

use aurorix_db::AppConfig;
use aurorix_gateway::{
    bits_for_names, fixed_headers, requires_websocket, GatewayConfig, DEFAULT_GATEWAY_URL,
};
use aurorix_log::Level;
use aurorix_render::SchedulerConfig;
use serde_json::Value;

use crate::error::BotError;
use crate::lock::{DEFAULT_LOCK_FILE, LOCK_FILE_ENV};

pub const DEFAULT_ADMIN_PORT: u16 = 8080;

pub const DEFAULT_ADMIN_LISTEN: &str = "0.0.0.0";

pub const DEFAULT_RENDER_POOL: usize = 4;

pub const DEFAULT_LOG_LEVEL: &str = "info";

pub const DEFAULT_PROXY_API: &str = "https://api.bot.qq.com";

pub const DEFAULT_PROTOCOL: &str = "websocket";

pub const LOG_LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

#[derive(Debug, Clone, PartialEq)]
pub struct BotConfig {
    pub appid: String,
    pub secret: String,
    pub intents: Vec<String>,
    pub gateway_url: Option<String>,
    pub proxy_api: String,
    pub database: String,
    pub admin_listen: String,
    pub admin_port: u16,
    pub admin_password: String,
    pub render_pool: usize,
    pub log_level: String,
    pub protocol: String,
    pub prefixes: Vec<String>,
    pub maintenance: Option<String>,
    pub plugin_settings: BTreeMap<String, BTreeMap<String, Value>>,
    pub plugin_access: BTreeMap<String, aurorix_db::AccessConfig>,
    pub lock_file: String,
    pub upload_threshold: usize,
    pub retry_when: Vec<i32>,
    pub image_max_width: Option<u32>,
    pub image_max_height: Option<u32>,
    pub image_limits_enabled: Option<bool>,
    pub global_markdown: bool,
    pub panel_sync: bool,
    pub keyboard_font_size: Option<String>,
    pub footer_buttons_enabled: Option<bool>,
    pub footer_buttons: Option<Vec<String>>,
    pub footer_buttons_group: Option<Vec<String>>,
    pub footer_buttons_c2c: Option<Vec<String>>,
    pub footer_buttons_channel: Option<Vec<String>>,
    pub welcome_enabled: Option<bool>,
    pub welcome_messages: Option<Vec<String>>,
    pub welcome_extra: Option<String>,
    pub welcome_buttons: Option<Vec<String>>,
    pub farewell_enabled: Option<bool>,
    pub farewell_messages: Option<Vec<String>>,
    pub farewell_extra: Option<String>,
    pub farewell_buttons: Option<Vec<String>>,
    pub footer_buttons_dm: Option<Vec<String>>,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            appid: String::new(),
            secret: String::new(),
            intents: aurorix_db::config::default_intents(),
            gateway_url: None,
            proxy_api: String::from(DEFAULT_PROXY_API),
            database: String::from(aurorix_db::DEFAULT_DATABASE),
            admin_listen: String::from(DEFAULT_ADMIN_LISTEN),
            admin_port: DEFAULT_ADMIN_PORT,
            admin_password: String::new(),
            render_pool: DEFAULT_RENDER_POOL,
            log_level: String::from(DEFAULT_LOG_LEVEL),
            protocol: String::from(DEFAULT_PROTOCOL),
            prefixes: aurorix_core::prefix::default_prefix_chars(),
            maintenance: None,
            plugin_settings: BTreeMap::new(),
            plugin_access: BTreeMap::new(),
            lock_file: String::from(DEFAULT_LOCK_FILE),
            upload_threshold: aurorix_core::DEFAULT_UPLOAD_THRESHOLD,
            retry_when: aurorix_protocol::DEFAULT_RETRY_WHEN.to_vec(),
            image_max_width: None,
            image_max_height: None,
            image_limits_enabled: None,
            global_markdown: false,
            panel_sync: false,
            keyboard_font_size: None,
            footer_buttons_enabled: None,
            footer_buttons: None,
            footer_buttons_group: None,
            footer_buttons_c2c: None,
            footer_buttons_channel: None,
            footer_buttons_dm: None,
            welcome_enabled: None,
            welcome_messages: None,
            welcome_extra: None,
            welcome_buttons: None,
            farewell_enabled: None,
            farewell_messages: None,
            farewell_extra: None,
            farewell_buttons: None,
        }
    }
}

impl BotConfig {
    pub fn new(appid: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            appid: appid.into(),
            secret: secret.into(),
            ..Self::default()
        }
    }

    pub fn adaptive_render_pool() -> usize {
        aurorix_core::HardwareProfile::cached().render_workers()
    }

    pub fn from_app_config(app: &AppConfig) -> Self {
        let mut source = app.clone();
        source.normalize();
        let proxy_api = if source.proxy.trim().is_empty() {
            String::from(DEFAULT_PROXY_API)
        } else {
            source.proxy.trim().to_string()
        };
        let maintenance = match source.bot_maintenance.unwrap_or(false) {
            true => Some(source.maintenance_message().to_string()),
            false => None,
        };
        Self {
            appid: source.appid.trim().to_string(),
            secret: source.secret.trim().to_string(),
            intents: source.intents(),
            gateway_url: source
                .gateway_url
                .clone()
                .map(|url| url.trim().to_string())
                .filter(|url| !url.is_empty()),
            proxy_api,
            database: source.database_url().to_string(),
            admin_listen: source.listen_addr().to_string(),
            admin_port: if source.port == 0 {
                DEFAULT_ADMIN_PORT
            } else {
                source.port
            },
            admin_password: source.admin_password.clone(),
            render_pool: source
                .render_pool
                .unwrap_or_else(BotConfig::adaptive_render_pool)
                .max(1),
            log_level: source.log_level().to_string(),
            protocol: source.protocol().to_string(),
            prefixes: source.prefixes(),
            maintenance,
            plugin_settings: source.plugin_settings.clone().unwrap_or_default(),
            plugin_access: source.plugin_access.clone().unwrap_or_default(),
            lock_file: String::from(DEFAULT_LOCK_FILE),
            upload_threshold: source
                .upload_threshold
                .unwrap_or(aurorix_core::DEFAULT_UPLOAD_THRESHOLD as i64)
                .max(1) as usize,
            retry_when: source
                .retry_when
                .clone()
                .unwrap_or_else(|| aurorix_protocol::DEFAULT_RETRY_WHEN.to_vec()),
            image_max_width: source.image_max_width,
            image_max_height: source.image_max_height,
            image_limits_enabled: source.image_limits_enabled,
            global_markdown: source.global_markdown.unwrap_or(false),
            panel_sync: source.panel_sync.unwrap_or(false),
            keyboard_font_size: source.keyboard_font_size.clone(),
            footer_buttons_enabled: source.footer_buttons_enabled,
            footer_buttons: source.footer_buttons.clone(),
            footer_buttons_group: source.footer_buttons_group.clone(),
            footer_buttons_c2c: source.footer_buttons_c2c.clone(),
            footer_buttons_channel: source.footer_buttons_channel.clone(),
            footer_buttons_dm: source.footer_buttons_dm.clone(),
            welcome_enabled: source.welcome_enabled,
            welcome_messages: source.welcome_messages.clone(),
            welcome_extra: source.welcome_extra.clone(),
            welcome_buttons: source.welcome_buttons.clone(),
            farewell_enabled: source.farewell_enabled,
            farewell_messages: source.farewell_messages.clone(),
            farewell_extra: source.farewell_extra.clone(),
            farewell_buttons: source.farewell_buttons.clone(),
        }
    }

    pub fn with_gateway_url(mut self, url: impl Into<String>) -> Self {
        self.gateway_url = Some(url.into());
        self
    }

    pub fn with_proxy_api(mut self, url: impl Into<String>) -> Self {
        self.proxy_api = url.into();
        self
    }

    pub fn with_admin(mut self, listen: impl Into<String>, port: u16) -> Self {
        self.admin_listen = listen.into();
        self.admin_port = port;
        self
    }

    pub fn with_render_pool(mut self, workers: usize) -> Self {
        self.render_pool = workers;
        self
    }

    pub fn with_intents(mut self, intents: Vec<String>) -> Self {
        self.intents = intents;
        self
    }

    pub fn with_maintenance(mut self, text: Option<String>) -> Self {
        self.maintenance = text;
        self
    }

    pub fn image_box(&self) -> aurorix_api::ImageBox {
        if self.image_limits_enabled == Some(false) {
            return aurorix_api::ImageBox::off();
        }
        aurorix_api::ImageBox::new(
            self.image_max_width
                .unwrap_or(aurorix_core::DEFAULT_IMAGE_MAX_WIDTH),
            self.image_max_height
                .unwrap_or(aurorix_core::DEFAULT_IMAGE_MAX_HEIGHT),
        )
    }

    pub fn footer_enabled(&self) -> bool {
        self.footer_buttons_enabled.unwrap_or(true)
    }

    pub fn footer_global_lines(&self) -> Vec<String> {
        if !self.footer_enabled() {
            return Vec::new();
        }
        self.footer_buttons.clone().unwrap_or_default()
    }

    pub fn footer_scene_lines(&self, scene: aurorix_core::Scene) -> Vec<String> {
        if !self.footer_enabled() {
            return Vec::new();
        }
        let scoped = match scene {
            aurorix_core::Scene::Group => &self.footer_buttons_group,
            aurorix_core::Scene::C2c => &self.footer_buttons_c2c,
            aurorix_core::Scene::Channel => &self.footer_buttons_channel,
            aurorix_core::Scene::Dm => &self.footer_buttons_dm,
        };
        scoped.clone().unwrap_or_default()
    }

    pub fn welcome_on(&self) -> bool {
        self.welcome_enabled.unwrap_or(true)
    }

    pub fn farewell_on(&self) -> bool {
        self.farewell_enabled.unwrap_or(true)
    }

    pub fn welcome_lines(&self) -> Vec<String> {
        self.welcome_messages.clone().unwrap_or_default()
    }

    pub fn farewell_lines(&self) -> Vec<String> {
        self.farewell_messages.clone().unwrap_or_default()
    }

    pub fn welcome_extra_text(&self) -> &str {
        self.welcome_extra.as_deref().unwrap_or("")
    }

    pub fn farewell_extra_text(&self) -> &str {
        self.farewell_extra.as_deref().unwrap_or("")
    }

    pub fn welcome_button_lines(&self) -> Vec<String> {
        self.welcome_buttons.clone().unwrap_or_default()
    }

    pub fn farewell_button_lines(&self) -> Vec<String> {
        self.farewell_buttons.clone().unwrap_or_default()
    }

    pub fn with_footer_buttons(mut self, lines: Vec<String>) -> Self {
        self.footer_buttons = Some(lines);
        self
    }

    pub fn with_lock_file(mut self, path: impl Into<String>) -> Self {
        self.lock_file = path.into();
        self
    }

    pub fn lock_file_path(&self) -> String {
        match std::env::var(LOCK_FILE_ENV) {
            Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => self.lock_file.clone(),
        }
    }

    pub fn admin_addr(&self) -> String {
        format!("{}:{}", self.admin_listen, self.admin_port)
    }

    pub fn intent_bits(&self) -> u64 {
        bits_for_names(&self.intents)
    }

    pub fn needs_websocket(&self) -> bool {
        requires_websocket(&self.intents)
    }

    pub fn level(&self) -> Result<Level, BotError> {
        Level::parse(&self.log_level).map_err(|err| {
            BotError::Config(format!(
                "log_level 只能是 {:?}, 收到 {:?}: {err}",
                LOG_LEVELS, self.log_level
            ))
        })
    }

    pub fn validate(&self) -> Result<(), BotError> {
        if self.appid.trim().is_empty() {
            return Err(BotError::ConfigMissing {
                field: "appid",
                hint: "config.json 的 appid, 即 QQ 开放平台的机器人 AppID",
            });
        }
        if self.secret.trim().is_empty() {
            return Err(BotError::ConfigMissing {
                field: "secret",
                hint: "config.json 的 secret, 用于换 access token 与 webhook 签名",
            });
        }
        if self.protocol != "webhook" && self.protocol != "websocket" {
            return Err(BotError::Config(format!(
                "protocol 只能是 webhook 或 websocket, 收到 {:?}",
                self.protocol
            )));
        }
        if self.admin_port == 0 {
            return Err(BotError::Config(String::from(
                "admin_port 不能为 0, 管理台需要一个可用端口",
            )));
        }
        self.level()?;
        Ok(())
    }

    pub fn gateway_config(&self) -> GatewayConfig {
        let url = self
            .gateway_url
            .clone()
            .unwrap_or_else(|| String::from(DEFAULT_GATEWAY_URL));
        GatewayConfig::new(url, self.intent_bits()).with_headers(fixed_headers(&self.appid))
    }

    pub fn api_config(&self) -> aurorix_api::BotConfig {
        aurorix_api::BotConfig::new(self.appid.clone(), self.secret.clone())
            .with_base_url(self.proxy_api.clone())
    }

    pub fn scheduler_config(&self) -> SchedulerConfig {
        SchedulerConfig {
            workers: self.render_pool.max(1),
            ..SchedulerConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maintenance_switch_survives_a_reload() {
        let dir =
            std::env::temp_dir().join(format!("aurorix-bot-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"appid":"app-1","secret":"secret-1"}"#).expect("写配置");
        let mut app = AppConfig::load(&path).expect("读配置");
        assert!(BotConfig::from_app_config(&app).maintenance.is_none());
        app.apply_core_key("bot_maintenance", &json!(true))
            .expect("开维护");
        app.apply_core_key("bot_maintenance_message", &json!("维护中, 请稍后再试"))
            .expect("维护文案");
        app.persist(&path, &["bot_maintenance", "bot_maintenance_message"])
            .expect("落盘");
        let reloaded = AppConfig::load(&path).expect("重新加载");
        assert_eq!(
            BotConfig::from_app_config(&reloaded).maintenance.as_deref(),
            Some("维护中, 请稍后再试")
        );
        app.apply_core_key("bot_maintenance", &json!(false))
            .expect("关维护");
        app.persist(&path, &["bot_maintenance"]).expect("落盘");
        let reloaded = AppConfig::load(&path).expect("重新加载");
        assert!(BotConfig::from_app_config(&reloaded).maintenance.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
