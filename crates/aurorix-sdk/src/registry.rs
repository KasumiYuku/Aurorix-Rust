use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use arc_swap::ArcSwap;

use crate::plugin::{Plugin, PluginMeta};

#[derive(Clone)]
struct Slot {
    plugin: Arc<dyn Plugin>,
    meta: PluginMeta,
}

pub struct PluginRegistry {
    state: ArcSwap<BTreeMap<&'static str, Slot>>,
    write_lock: Mutex<()>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("plugins", &self.len())
            .finish()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            state: ArcSwap::from_pointee(BTreeMap::new()),
            write_lock: Mutex::new(()),
        }
    }

    pub fn global() -> &'static Self {
        static REGISTRY: LazyLock<PluginRegistry> = LazyLock::new(PluginRegistry::new);
        &REGISTRY
    }

    pub fn register(&self, plugin: Box<dyn Plugin>) -> Option<PluginMeta> {
        let meta = plugin.meta();
        let id = plugin.id();
        let plugin: Arc<dyn Plugin> = Arc::from(plugin);
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = BTreeMap::clone(self.state.load_full().as_ref());
        let previous = next.insert(id, Slot { plugin, meta }).map(|slot| slot.meta);
        self.state.store(Arc::new(next));
        previous
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Plugin>> {
        self.state
            .load_full()
            .get(id)
            .map(|slot| Arc::clone(&slot.plugin))
    }

    pub fn meta(&self, id: &str) -> Option<PluginMeta> {
        self.state.load_full().get(id).map(|slot| slot.meta.clone())
    }

    pub fn ids(&self) -> Vec<String> {
        self.state
            .load_full()
            .keys()
            .map(|id| (*id).to_string())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.state.load_full().len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.load_full().is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.state.load_full().contains_key(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Demo {
        id: &'static str,
    }

    impl Plugin for Demo {
        fn id(&self) -> &'static str {
            self.id
        }
    }

    struct Described {
        id: &'static str,
    }

    impl Plugin for Described {
        fn id(&self) -> &'static str {
            self.id
        }

        fn describe(&self) -> String {
            format!("{} 插件", self.id)
        }

        fn meta(&self) -> PluginMeta {
            PluginMeta::new(self.id, self.describe(), "1.2.3")
        }
    }

    fn demo(id: &'static str) -> Box<dyn Plugin> {
        Box::new(Demo { id })
    }

    #[test]
    fn register_then_get_returns_same_plugin() {
        let registry = PluginRegistry::new();
        assert!(registry.is_empty());
        assert!(registry.register(demo("arcaea")).is_none());
        assert_eq!(
            registry.get("arcaea").map(|plugin| plugin.id()),
            Some("arcaea")
        );
        assert!(registry.contains("arcaea"));
        assert_eq!(registry.len(), 1);
        assert!(registry.get("missing").is_none(), "不存在的插件要返回 None");
        assert_eq!(registry.meta("missing"), None);
    }

    #[test]
    fn default_describe_and_meta_follow_id() {
        let registry = PluginRegistry::new();
        registry.register(demo("plain"));
        let plugin = registry.get("plain").expect("插件要注册成功");
        assert_eq!(plugin.describe(), "plain");
        let meta = registry.meta("plain").expect("元数据要存在");
        assert_eq!(meta.id, "plain");
        assert_eq!(meta.name, "plain");
        assert_eq!(meta.version, PluginMeta::UNKNOWN_VERSION);
    }

    #[test]
    fn custom_describe_and_meta_are_reported() {
        let registry = PluginRegistry::new();
        let previous = registry.register(Box::new(Described { id: "fancy" }));
        assert!(previous.is_none());
        let meta = registry.meta("fancy").expect("元数据要存在");
        assert_eq!(meta.name, "fancy 插件");
        assert_eq!(meta.version, "1.2.3");
        assert_eq!(
            registry.get("fancy").map(|plugin| plugin.describe()),
            Some("fancy 插件".to_string())
        );
    }

    #[test]
    fn duplicate_registration_reports_previous_meta() {
        let registry = PluginRegistry::new();
        registry.register(demo("dup"));
        let previous = registry
            .register(demo("dup"))
            .expect("重复注册要报旧元数据");
        assert_eq!(previous.id, "dup");
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.ids(), vec!["dup".to_string()]);
    }

    #[test]
    fn ids_are_sorted() {
        let registry = PluginRegistry::new();
        for id in ["zeta", "alpha", "mid"] {
            registry.register(demo(id));
        }
        assert_eq!(
            registry.ids(),
            vec!["alpha".to_string(), "mid".to_string(), "zeta".to_string()],
            "id 列表要排序"
        );
    }

    #[test]
    fn concurrent_registration_keeps_every_plugin() {
        let registry = Arc::new(PluginRegistry::new());
        let mut handles = Vec::new();
        for index in 0..100u32 {
            let registry = Arc::clone(&registry);
            handles.push(std::thread::spawn(move || {
                let id: &'static str = Box::leak(format!("plugin-{index:03}").into_boxed_str());
                registry.register(Box::new(Demo { id }));
                assert!(registry.get(id).is_some(), "注册后要能读到");
            }));
        }
        for handle in handles {
            handle.join().expect("线程不该 panic");
        }
        assert_eq!(registry.len(), 100);
        assert_eq!(registry.ids().len(), 100);
        assert!(registry.get("plugin-000").is_some());
        assert!(registry.get("plugin-099").is_some());
    }
}
