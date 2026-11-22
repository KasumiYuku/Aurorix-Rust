use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use aurorix_db::AppConfig;
use serde_json::Value;

use crate::ports::PluginConfigurable;

pub const MODULE: &str = "plugin_config";

pub struct PluginConfigStore {
    path: Option<PathBuf>,
    values: Mutex<BTreeMap<String, BTreeMap<String, Value>>>,
    sinks: Mutex<Vec<Arc<dyn PluginConfigurable>>>,
}

impl std::fmt::Debug for PluginConfigStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginConfigStore")
            .field("path", &self.path)
            .field("plugins", &lock(&self.values).len())
            .field("sinks", &lock(&self.sinks).len())
            .finish()
    }
}

impl PluginConfigStore {
    pub fn new(path: Option<PathBuf>, values: BTreeMap<String, BTreeMap<String, Value>>) -> Self {
        Self {
            path,
            values: Mutex::new(values),
            sinks: Mutex::new(Vec::new()),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn attach(&self, sink: Arc<dyn PluginConfigurable>) {
        let mut sinks = lock(&self.sinks);
        if sinks
            .iter()
            .any(|existing| existing.plugin_id() == sink.plugin_id())
        {
            return;
        }
        sinks.push(sink);
    }

    pub fn sinks(&self) -> Vec<Arc<dyn PluginConfigurable>> {
        lock(&self.sinks).clone()
    }

    pub fn values(&self, plugin_id: &str) -> BTreeMap<String, Value> {
        lock(&self.values)
            .get(plugin_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn stored(&self) -> BTreeMap<String, BTreeMap<String, Value>> {
        lock(&self.values).clone()
    }

    pub fn apply(&self, plugin_id: &str, values: &BTreeMap<String, Value>) -> Vec<String> {
        let mut failures = Vec::new();
        for sink in self.sinks() {
            if sink.plugin_id() != plugin_id {
                continue;
            }
            if let Err(error) = sink.apply(values) {
                failures.push(format!("{}: {error}", sink.plugin_id()));
            }
        }
        failures
    }

    pub fn save(&self, plugin_id: &str, values: BTreeMap<String, Value>) -> Result<(), String> {
        let failures = self.apply(plugin_id, &values);
        if !failures.is_empty() {
            return Err(failures.join("; "));
        }
        lock(&self.values).insert(plugin_id.to_string(), values.clone());
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        persist(&path, plugin_id, values)
    }

    pub fn apply_all(&self) -> Vec<String> {
        let stored = self.stored();
        let mut failures = Vec::new();
        for sink in self.sinks() {
            let plugin_id = sink.plugin_id().to_string();
            let settings = stored.get(&plugin_id).cloned().unwrap_or_default();
            if let Err(error) = sink.apply(&settings) {
                failures.push(format!("{plugin_id}: {error}"));
            }
        }
        failures
    }
}

fn persist(path: &Path, plugin_id: &str, values: BTreeMap<String, Value>) -> Result<(), String> {
    let mut config = AppConfig::load(path)
        .map_err(|error| format!("配置已生效, 但读取配置文件失败: {error}"))?;
    config
        .save_plugin_settings(path, plugin_id, values)
        .map_err(|error| format!("配置已生效, 但写回配置文件失败: {error}"))
}

fn lock<T>(target: &Mutex<T>) -> MutexGuard<'_, T> {
    target
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::ClosurePluginConfig;
    use serde_json::json;

    fn values(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn save_reaches_only_the_matching_sink() {
        let store = PluginConfigStore::new(None, BTreeMap::new());
        let seen = Arc::new(Mutex::new(Vec::<BTreeMap<String, Value>>::new()));
        let first = Arc::clone(&seen);
        store.attach(Arc::new(ClosurePluginConfig::new(
            "arcaea",
            move |settings| {
                lock(&first).push(settings.clone());
                Ok(())
            },
        )));
        let other = Arc::new(Mutex::new(0usize));
        let counter = Arc::clone(&other);
        store.attach(Arc::new(ClosurePluginConfig::new("other", move |_| {
            *lock(&counter) += 1;
            Ok(())
        })));
        store
            .save("arcaea", values(&[("token", json!("abc"))]))
            .expect("保存要成功");
        assert_eq!(lock(&seen).len(), 1, "命中的插件要收到一次配置");
        assert_eq!(lock(&seen)[0].get("token"), Some(&json!("abc")));
        assert_eq!(*lock(&other), 0, "其它插件不该被调用");
    }

    #[test]
    fn save_reports_sink_failure_without_committing() {
        let store = PluginConfigStore::new(None, BTreeMap::new());
        store.attach(Arc::new(ClosurePluginConfig::new("arcaea", |_| {
            Err(String::from("令牌无效"))
        })));
        let error = store
            .save("arcaea", values(&[("token", json!("abc"))]))
            .expect_err("插件拒绝时要报错");
        assert!(error.contains("令牌无效"), "错误要带上插件原因: {error}");
        assert!(store.values("arcaea").is_empty(), "失败时不能把值写进内存");
    }

    #[test]
    fn values_round_trip_after_save() {
        let store = PluginConfigStore::new(None, BTreeMap::new());
        store.attach(Arc::new(ClosurePluginConfig::new("arcaea", |_| Ok(()))));
        store
            .save("arcaea", values(&[("limit", json!(5))]))
            .expect("保存要成功");
        assert_eq!(store.values("arcaea").get("limit"), Some(&json!(5)));
    }

    #[test]
    fn duplicate_sinks_are_ignored() {
        let store = PluginConfigStore::new(None, BTreeMap::new());
        store.attach(Arc::new(ClosurePluginConfig::new("arcaea", |_| Ok(()))));
        store.attach(Arc::new(ClosurePluginConfig::new("arcaea", |_| Ok(()))));
        assert_eq!(store.sinks().len(), 1, "同一插件只保留一个配置出口");
    }

    #[test]
    fn apply_all_uses_stored_values() {
        let mut stored = BTreeMap::new();
        stored.insert(String::from("arcaea"), values(&[("limit", json!(7))]));
        let store = PluginConfigStore::new(None, stored);
        let seen = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
        let target = Arc::clone(&seen);
        store.attach(Arc::new(ClosurePluginConfig::new(
            "arcaea",
            move |settings| {
                *lock(&target) = settings.clone();
                Ok(())
            },
        )));
        let failures = store.apply_all();
        assert!(failures.is_empty(), "启动应用不该失败: {failures:?}");
        assert_eq!(lock(&seen).get("limit"), Some(&json!(7)));
    }
}
