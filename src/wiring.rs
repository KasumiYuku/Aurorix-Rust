use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use aurorix_bot::{ClosurePluginConfig, PluginConfigurable};
use aurorix_db::{Db, PluginDb, RegistryConfig};
use aurorix_plugin::PluginCommands;
use serde_json::Value;

pub type Settings = BTreeMap<String, BTreeMap<String, Value>>;

pub struct Wiring {
    pub plugins: Vec<PluginCommands>,
    pub sinks: Vec<Arc<dyn PluginConfigurable>>,
    pub notes: Vec<String>,
    pub phigros_webui: Option<aurorix_phigros_plugin::webui::WebuiService>,
}

const ARCAEA_DEFAULT_BACKEND: &str = "http://127.0.0.1:2388";

pub async fn build(db: &Db, settings: &Settings) -> Result<Wiring, String> {
    let registry = db.plugin_registry(RegistryConfig::default());
    let mut wiring = Wiring {
        plugins: Vec::new(),
        sinks: Vec::new(),
        notes: Vec::new(),
        phigros_webui: None,
    };

    {
        let plugin_db = open(&registry, "arcaea")?;
        let backend = Arc::new(
            aurorix_arcaea_plugin::backend::ArcBackendNext::new(arcaea_backend(settings))
                .map_err(|error| format!("arcaea 后端配置无效：{error}"))?,
        );
        let (commands, report) =
            aurorix_arcaea_plugin::install_with_report(plugin_db, backend).await?;
        wiring.notes.push(format!("arcaea 装配 {report}"));
        wiring.plugins.push(commands);
    }

    {
        let plugin_db = open(&registry, "phigros")?;
        let client = Arc::new(
            aurorix_phigros_plugin::backend::SaveClient::new()
                .map_err(|error| format!("phigros 存档目录不可用：{error}"))?,
        );
        let (commands, webui, report) =
            aurorix_phigros_plugin::install_with_report(plugin_db, client).await?;
        wiring.notes.push(format!("phigros 装配 {report}"));
        wiring.phigros_webui = Some(webui);
        wiring.plugins.push(commands);
    }

    {
        let plugin_db = open(&registry, "bottle")?;
        let (commands, handle, report) = aurorix_bottle_plugin::install_full(plugin_db).await?;
        wiring
            .sinks
            .push(Arc::new(ClosurePluginConfig::new("bottle", move |raw| {
                handle.apply(raw)
            })));
        wiring.notes.push(format!("bottle 装配 {report}"));
        wiring.plugins.push(commands);
    }

    {
        let plugin_db = open(&registry, "okamikko")?;
        let (commands, handle, report) = aurorix_okamikko_plugin::install_full(plugin_db).await?;
        wiring
            .sinks
            .push(Arc::new(ClosurePluginConfig::new("okamikko", move |raw| {
                handle.apply(raw)
            })));
        wiring.notes.push(format!("okamikko 装配 {report}"));
        wiring.plugins.push(commands);
    }

    {
        let plugin_db = open(&registry, "rollpig")?;
        let (commands, handle, report) = aurorix_rollpig_plugin::install_full(plugin_db).await?;
        wiring
            .sinks
            .push(Arc::new(ClosurePluginConfig::new("rollpig", move |raw| {
                handle.apply(raw)
            })));
        wiring.notes.push(format!("rollpig 装配 {report}"));
        wiring.plugins.push(commands);
    }

    {
        let plugin_db = open(&registry, "music")?;
        let (commands, report) = aurorix_music_plugin::install_with_report(plugin_db).await?;
        wiring.notes.push(format!("music 装配 {report}"));
        wiring.plugins.push(commands);
    }

    {
        let plugin_db = open(&registry, "link")?;
        let (commands, report) = aurorix_link_plugin::install_with_report(plugin_db).await?;
        wiring.notes.push(format!("link 装配 {report}"));
        wiring.plugins.push(commands);
    }

    Ok(wiring)
}

fn open(registry: &aurorix_db::PluginDbRegistry, plugin_id: &str) -> Result<Arc<PluginDb>, String> {
    registry
        .plugin(plugin_id)
        .map_err(|error| format!("{plugin_id} 插件库不可用：{error}"))
}

fn arcaea_backend(settings: &Settings) -> aurorix_arcaea_plugin::backend::BackendConfig {
    let base = text(settings, "arcaea", "backend_url");
    let base = if base.trim().is_empty() {
        ARCAEA_DEFAULT_BACKEND.to_string()
    } else {
        base
    };
    let mut config = aurorix_arcaea_plugin::backend::BackendConfig::new(base);
    let token = text(settings, "arcaea", "backend_token");
    if !token.trim().is_empty() {
        config = config.token(token);
    }
    let admin = text(settings, "arcaea", "admin_token");
    if !admin.trim().is_empty() {
        config = config.admin_token(admin);
    }
    if let Some(seconds) = number(settings, "arcaea", "timeout_seconds") {
        if seconds > 0 {
            config = config.timeout(Duration::from_secs(seconds));
        }
    }
    config
}

fn text(settings: &Settings, plugin: &str, key: &str) -> String {
    match settings.get(plugin).and_then(|fields| fields.get(key)) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn number(settings: &Settings, plugin: &str, key: &str) -> Option<u64> {
    let raw = text(settings, plugin, key);
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.parse::<u64>() {
        Ok(value) => Some(value),
        Err(_) => raw.parse::<f64>().ok().map(|value| value.max(0.0) as u64),
    }
}
