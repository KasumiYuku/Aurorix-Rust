use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;

use aurorix_core::{Scene, Target};

use crate::reply::{InvocationSender, Outgoing};

#[derive(Debug, Clone, PartialEq)]
pub struct CallbackRequest {
    pub button_id: String,
    pub data: String,
    pub plugin: String,
    pub user: String,
    pub scene: Scene,
    pub target: Target,
    pub sender: Option<InvocationSender>,
    pub interaction_id: String,
}

pub type CallbackFuture = Pin<Box<dyn Future<Output = Option<Outgoing>> + Send>>;

pub type CallbackHandler = Arc<dyn Fn(CallbackRequest) -> CallbackFuture + Send + Sync>;

struct Entry {
    plugin: String,
    handler: CallbackHandler,
}

#[derive(Default)]
pub struct CallbackBook {
    handlers: Mutex<HashMap<String, Entry>>,
}

impl std::fmt::Debug for CallbackBook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CallbackBook")
            .field("ids", &self.ids())
            .finish()
    }
}

fn inferred_plugin(id: &str) -> String {
    id.split('.').next().unwrap_or_default().to_string()
}

impl CallbackBook {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn global() -> &'static CallbackBook {
        static BOOK: LazyLock<CallbackBook> = LazyLock::new(CallbackBook::default);
        &BOOK
    }

    pub fn register(&self, id: impl Into<String>, handler: CallbackHandler) -> bool {
        let id = id.into();
        let plugin = inferred_plugin(&id);
        self.register_scoped(id, plugin, handler)
    }

    pub fn register_scoped(
        &self,
        id: impl Into<String>,
        plugin: impl Into<String>,
        handler: CallbackHandler,
    ) -> bool {
        let id = id.into();
        let mut guard = self
            .handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.contains_key(&id) {
            return false;
        }
        guard.insert(
            id,
            Entry {
                plugin: plugin.into(),
                handler,
            },
        );
        true
    }

    pub fn contains(&self, id: &str) -> bool {
        let guard = self
            .handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.contains_key(id)
    }

    pub fn ids(&self) -> Vec<String> {
        let guard = self
            .handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut ids: Vec<String> = guard.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn len(&self) -> usize {
        let guard = self
            .handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn plugin_of(&self, id: &str) -> Option<String> {
        let guard = self
            .handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.get(id).map(|entry| entry.plugin.clone())
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut guard = self
            .handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.remove(id).is_some()
    }

    pub fn dispatch(&self, request: CallbackRequest) -> Option<CallbackFuture> {
        let handler = {
            let guard = self
                .handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.get(&request.button_id)?.handler.clone()
        };
        Some(handler(request))
    }
}
