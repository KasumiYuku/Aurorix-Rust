use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::config::ProviderItem;
use crate::error::AssetError;
use crate::http::{HttpClient, ReqwestClient};
use crate::provider::Provider;

pub type ProviderFactory = Box<
    dyn Fn(&ProviderItem, Arc<dyn HttpClient>) -> Result<Arc<dyn Provider>, AssetError>
        + Send
        + Sync,
>;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(rename = "default", skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

impl ConfigField {
    pub fn new(key: &str, label: &str, field_type: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            description: String::new(),
            field_type: field_type.to_string(),
            required: false,
            placeholder: String::new(),
            default_value: None,
            options: Vec::new(),
        }
    }

    pub fn password(key: &str, label: &str) -> Self {
        Self::new(key, label, "password")
    }

    pub fn text(key: &str, label: &str) -> Self {
        Self::new(key, label, "text")
    }

    pub fn number(key: &str, label: &str) -> Self {
        Self::new(key, label, "number")
    }

    pub fn description(mut self, description: &str) -> Self {
        self.description = description.to_string();
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = placeholder.to_string();
        self
    }

    pub fn default_value(mut self, value: impl Into<Value>) -> Self {
        self.default_value = Some(value.into());
        self
    }

    pub fn options(mut self, options: &[&str]) -> Self {
        self.options = options.iter().map(|item| (*item).to_string()).collect();
        self
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderSpec {
    pub name: String,
    pub label: String,
    pub default_on: bool,
    pub probe_skip: bool,
    pub fields: Vec<ConfigField>,
}

pub struct ProviderEntry {
    pub name: &'static str,
    pub label: &'static str,
    pub default_on: bool,
    pub probe_skip: bool,
    pub fields: Vec<ConfigField>,
    pub factory: ProviderFactory,
}

impl ProviderEntry {
    pub fn new(
        name: &'static str,
        label: &'static str,
        fields: Vec<ConfigField>,
        factory: ProviderFactory,
    ) -> Self {
        Self {
            name,
            label,
            default_on: false,
            probe_skip: false,
            fields,
            factory,
        }
    }

    pub fn default_on(mut self) -> Self {
        self.default_on = true;
        self
    }

    pub fn probe_skip(mut self) -> Self {
        self.probe_skip = true;
        self
    }
}

pub struct ProviderRegistry {
    entries: Vec<ProviderEntry>,
    http: Arc<dyn HttpClient>,
}

impl ProviderRegistry {
    pub fn with_builtin() -> Self {
        Self::with_client(Arc::new(ReqwestClient::new()))
    }

    pub fn with_client(http: Arc<dyn HttpClient>) -> Self {
        let mut registry = Self::empty(http);
        for entry in crate::providers::builtin_entries() {
            registry.register(entry);
        }
        registry
    }

    pub fn empty(http: Arc<dyn HttpClient>) -> Self {
        Self {
            entries: Vec::new(),
            http,
        }
    }

    pub fn register(&mut self, entry: ProviderEntry) {
        self.entries.retain(|item| item.name != entry.name);
        self.entries.push(entry);
    }

    pub fn client(&self) -> &Arc<dyn HttpClient> {
        &self.http
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|entry| entry.name == name)
    }

    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.entries.iter().map(|entry| entry.name).collect();
        names.sort_unstable();
        names
    }

    pub fn specs(&self) -> Vec<ProviderSpec> {
        let mut specs: Vec<ProviderSpec> = self
            .entries
            .iter()
            .map(|entry| ProviderSpec {
                name: entry.name.to_string(),
                label: entry.label.to_string(),
                default_on: entry.default_on,
                probe_skip: entry.probe_skip,
                fields: entry.fields.clone(),
            })
            .collect();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    pub fn schema(&self, name: &str) -> Vec<ConfigField> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.fields.clone())
            .unwrap_or_default()
    }

    pub fn default_on_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .entries
            .iter()
            .filter(|entry| entry.default_on)
            .map(|entry| entry.name)
            .collect();
        names.sort_unstable();
        names
    }

    pub fn is_default_on(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.name == name && entry.default_on)
    }

    pub fn probe_skipped_by_default(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.name == name && entry.probe_skip)
    }

    pub fn instantiate(&self, item: &ProviderItem) -> Result<Arc<dyn Provider>, AssetError> {
        let Some(entry) = self.entries.iter().find(|entry| entry.name == item.name) else {
            return Err(AssetError::UnknownProvider(item.name.clone()));
        };
        (entry.factory)(item, Arc::clone(&self.http))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::MockHttp;
    use crate::testkit::ResultExt as _;

    fn registry() -> ProviderRegistry {
        ProviderRegistry::with_client(MockHttp::shared(Vec::new()))
    }

    #[test]
    fn builtin_registry_exposes_thirteen_providers() {
        let registry = registry();
        let names = registry.names();
        assert_eq!(names.len(), 13);
        assert_eq!(
            names,
            vec![
                "announce",
                "bilibili",
                "chatglm",
                "chinaunicom",
                "cnb",
                "homework",
                "kurobbs",
                "mihoyo",
                "qqbot",
                "qqgame",
                "qqstream",
                "qzone",
                "sgame",
            ]
        );
        assert_eq!(registry.default_on_names(), vec!["qqbot"]);
        assert!(registry.probe_skipped_by_default("qqbot"));
        assert!(registry.probe_skipped_by_default("cnb"));
        assert!(registry.probe_skipped_by_default("kurobbs"));
        assert!(!registry.probe_skipped_by_default("sgame"));
        assert!(registry.is_default_on("qqbot"));
        assert!(!registry.is_default_on("chatglm"));
    }

    #[test]
    fn specs_serialize_with_go_json_tags() {
        let registry = registry();
        let specs = registry.specs();
        assert_eq!(specs.len(), 13);
        let cnb = specs
            .iter()
            .find(|spec| spec.name == "cnb")
            .expect("cnb 应当存在");
        let raw = serde_json::to_value(cnb).unwrap();
        let fields = raw["fields"].as_array().unwrap();
        assert_eq!(fields[0]["key"], "token");
        assert_eq!(fields[0]["type"], "password");
        assert_eq!(fields[0]["required"], true);
        assert!(fields[0]["description"].is_string());
        assert_eq!(fields[1]["placeholder"], "用户名/仓库名");
        assert_eq!(fields[2]["default"], "https://api.cnb.cool");
        assert_eq!(fields[3]["type"], "number");
        assert_eq!(fields[3]["default"], 30);
        assert!(raw["label"].is_string());
        assert_eq!(raw["default_on"], false);
        assert_eq!(raw["probe_skip"], true);
    }

    #[test]
    fn schema_lookup_and_unknown_provider() {
        let registry = registry();
        assert!(registry.schema("unknown").is_empty());
        assert_eq!(registry.schema("kurobbs").len(), 1);
        let item = ProviderItem::new("unknown");
        let err = registry.instantiate(&item).failure();
        assert!(matches!(err, AssetError::UnknownProvider(_)));
    }

    #[test]
    fn instantiate_reports_missing_required_config() {
        let registry = registry();
        let err = registry.instantiate(&ProviderItem::new("bilibili")).failure();
        assert!(err.to_string().contains("SESSDATA"));
        let ok = registry
            .instantiate(
                &ProviderItem::new("bilibili")
                    .with_config("sessdata", "a")
                    .with_config("biliJct", "b"),
            )
            .expect("配置齐全应当构造成功");
        assert_eq!(ok.name(), "bilibili");
    }

    #[test]
    fn sgame_builds_from_the_built_in_secrets() {
        let registry = registry();
        let ok = registry
            .instantiate(&ProviderItem::new("sgame"))
            .expect("sgame 用内置密钥, 不需要用户填");
        assert_eq!(ok.name(), "sgame");
        assert_eq!(ok.kinds().bits(), 3);
    }

    #[test]
    fn custom_entries_can_be_registered() {
        let mut registry = ProviderRegistry::empty(MockHttp::shared(Vec::new()));
        assert!(registry.is_empty());
        registry.register(ProviderEntry::new(
            "fake",
            "假图床",
            vec![ConfigField::text("a", "A")],
            Box::new(|_item, _http| Ok(Arc::new(crate::testkit::TestProvider::new("fake")))),
        ));
        assert_eq!(registry.names(), vec!["fake"]);
        assert!(registry.contains("fake"));
        assert_eq!(registry.len(), 1);
        assert!(registry.instantiate(&ProviderItem::new("fake")).is_ok());
    }
}
