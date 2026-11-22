use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Scene {
    #[default]
    Group,
    C2c,
    Channel,
    Dm,
}

impl Scene {
    pub const ALL: [Scene; 4] = [Scene::Group, Scene::C2c, Scene::Channel, Scene::Dm];

    pub const fn as_str(self) -> &'static str {
        match self {
            Scene::Group => "group",
            Scene::C2c => "c2c",
            Scene::Channel => "channel",
            Scene::Dm => "dm",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Scene::Group => "群聊",
            Scene::C2c => "私聊",
            Scene::Channel => "频道",
            Scene::Dm => "频道私信",
        }
    }

    pub const fn guild(self) -> bool {
        matches!(self, Scene::Channel | Scene::Dm)
    }

    pub fn valid(self) -> bool {
        Scene::ALL.contains(&self)
    }
}

impl fmt::Display for Scene {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Scene {
    type Err = CoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "group" => Ok(Scene::Group),
            "c2c" => Ok(Scene::C2c),
            "channel" => Ok(Scene::Channel),
            "dm" => Ok(Scene::Dm),
            other => Err(CoreError::UnknownScene(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub scene: Scene,
    pub id: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub mention_id: String,
}

impl Target {
    pub fn new(scene: Scene, id: impl Into<String>) -> Self {
        Self {
            scene,
            id: id.into(),
            user: String::new(),
            mention_id: String::new(),
        }
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    pub fn with_mention_id(mut self, mention_id: impl Into<String>) -> Self {
        self.mention_id = mention_id.into();
        self
    }

    pub fn mention(&self) -> &str {
        if self.mention_id.is_empty() {
            &self.user
        } else {
            &self.mention_id
        }
    }

    pub fn empty(&self) -> bool {
        self.id.is_empty()
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.scene, self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_wire_names_are_stable() {
        assert_eq!(Scene::Group.as_str(), "group");
        assert_eq!(Scene::C2c.as_str(), "c2c");
        assert_eq!(Scene::Channel.as_str(), "channel");
        assert_eq!(Scene::Dm.as_str(), "dm");
        assert_eq!("c2c".parse::<Scene>().unwrap(), Scene::C2c);
        assert_eq!(Scene::C2c.to_string(), "c2c");
    }

    #[test]
    fn scene_labels_match_go_table() {
        assert_eq!(Scene::Group.label(), "群聊");
        assert_eq!(Scene::C2c.label(), "私聊");
        assert_eq!(Scene::Channel.label(), "频道");
        assert_eq!(Scene::Dm.label(), "频道私信");
    }

    #[test]
    fn unknown_scene_is_rejected() {
        let err = "wechat".parse::<Scene>().unwrap_err();
        assert_eq!(err, CoreError::UnknownScene("wechat".to_string()));
    }

    #[test]
    fn guild_only_covers_channel_domain() {
        assert!(Scene::Channel.guild());
        assert!(Scene::Dm.guild());
        assert!(!Scene::Group.guild());
        assert!(!Scene::C2c.guild());
    }

    #[test]
    fn mention_prefers_mention_id() {
        let target = Target::new(Scene::Channel, "c1")
            .with_user("union_openid")
            .with_mention_id("10086");
        assert_eq!(target.mention(), "10086");
        let fallback = Target::new(Scene::Group, "g1").with_user("openid");
        assert_eq!(fallback.mention(), "openid");
    }

    #[test]
    fn target_display_matches_go_key() {
        let target = Target::new(Scene::Group, "123");
        assert_eq!(target.to_string(), "group/123");
        assert!(!target.empty());
        assert!(Target::new(Scene::Group, "").empty());
    }

    #[test]
    fn scene_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Scene::C2c).unwrap(), "\"c2c\"");
        assert_eq!(serde_json::to_string(&Scene::Dm).unwrap(), "\"dm\"");
    }
}
