#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    pub version: String,
}

impl PluginMeta {
    pub const UNKNOWN_VERSION: &'static str = "unknown";

    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
        }
    }
}

pub trait Plugin: Send + Sync {
    fn id(&self) -> &'static str;

    fn describe(&self) -> String {
        self.id().to_string()
    }

    fn meta(&self) -> PluginMeta {
        PluginMeta::new(self.id(), self.describe(), PluginMeta::UNKNOWN_VERSION)
    }
}
