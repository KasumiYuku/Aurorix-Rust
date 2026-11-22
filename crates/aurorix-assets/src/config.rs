use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HostConfig {
    #[serde(default)]
    pub providers: Vec<ProviderItem>,
    #[serde(default)]
    pub whitelist: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderItem {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<bool>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub config: Map<String, Value>,
}

impl ProviderItem {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    pub fn with_probe(mut self, probe: bool) -> Self {
        self.probe = Some(probe);
        self
    }

    pub fn with_config(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.config.insert(key.to_string(), value.into());
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled != Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_config_parses_go_assets_json() {
        let raw = r#"{
            "providers": [
                {"name": "sgame", "priority": 30, "config": {"secretId": "a", "secretKey": "b"}},
                {"name": "kurobbs", "enabled": false, "probe": true, "config": {"token": "t"}}
            ],
            "whitelist": ["https://img.example.com/"]
        }"#;
        let cfg: HostConfig = serde_json::from_str(raw).expect("应当能解析 assets.json");
        assert_eq!(cfg.providers.len(), 2);
        assert_eq!(cfg.providers[0].priority, 30);
        assert_eq!(cfg.providers[0].enabled, None);
        assert_eq!(cfg.providers[1].enabled, Some(false));
        assert_eq!(cfg.providers[1].probe, Some(true));
        assert_eq!(
            cfg.providers[0].config.get("secretId"),
            Some(&Value::String("a".to_string()))
        );
        assert_eq!(cfg.whitelist, vec!["https://img.example.com/".to_string()]);
    }

    #[test]
    fn missing_fields_use_go_defaults() {
        let cfg: HostConfig = serde_json::from_str(r#"{"providers":[{"name":"qqgame"}]}"#).unwrap();
        let item = &cfg.providers[0];
        assert_eq!(item.priority, 0);
        assert_eq!(item.enabled, None);
        assert_eq!(item.probe, None);
        assert!(item.config.is_empty());
        assert!(cfg.whitelist.is_empty());
        assert!(item.is_enabled());
        assert!(!item.clone().with_enabled(false).is_enabled());
    }

    #[test]
    fn serialization_omits_empty_optional_fields() {
        let item = ProviderItem::new("chatglm");
        let raw = serde_json::to_string(&item).unwrap();
        assert_eq!(raw, r#"{"name":"chatglm","priority":0}"#);
        let full = ProviderItem::new("sgame")
            .with_priority(10)
            .with_enabled(true)
            .with_probe(false)
            .with_config("host", "h");
        let raw = serde_json::to_string(&full).unwrap();
        assert!(raw.contains(r#""enabled":true"#));
        assert!(raw.contains(r#""probe":false"#));
        assert!(raw.contains(r#""config":{"host":"h"}"#));
    }
}
