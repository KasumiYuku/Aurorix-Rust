use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;

#[derive(Default)]
pub struct CallbackRegistry {
    handlers: RwLock<BTreeMap<String, Arc<dyn Any + Send + Sync>>>,
}

impl std::fmt::Debug for CallbackRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackRegistry")
            .field("ids", &self.ids())
            .finish()
    }
}

impl CallbackRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn global() -> &'static CallbackRegistry {
        static REGISTRY: LazyLock<CallbackRegistry> = LazyLock::new(CallbackRegistry::new);
        &REGISTRY
    }

    pub fn register(&self, id: &str, handler: Arc<dyn Any + Send + Sync>) -> bool {
        let mut guard = self
            .handlers
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(id.to_string(), handler).is_some()
    }

    pub fn has(&self, id: &str) -> bool {
        let guard = self
            .handlers
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.contains_key(id)
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        let guard = self
            .handlers
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        let guard = self
            .handlers
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.keys().cloned().collect()
    }

    pub fn clear(&self) {
        let mut guard = self
            .handlers
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let registry = CallbackRegistry::new();
        assert!(!registry.has("x"));
        assert!(!registry.register("x", Arc::new(1u8)));
        assert!(registry.register("x", Arc::new(2u8)));
        assert!(registry.has("x"));
        assert_eq!(registry.ids(), vec!["x".to_string()]);
        registry.clear();
        assert!(!registry.has("x"));
    }
}
