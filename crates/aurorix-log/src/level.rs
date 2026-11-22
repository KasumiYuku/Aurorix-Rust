use std::fmt;

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::LogError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

pub const LEVELS: [Level; 4] = [Level::Debug, Level::Info, Level::Warn, Level::Error];

impl Level {
    pub fn parse(text: &str) -> Result<Self, LogError> {
        let trimmed = text.trim();
        let normalized = trimmed.to_lowercase();
        match normalized.as_str() {
            "debug" => Ok(Level::Debug),
            "" | "info" => Ok(Level::Info),
            "warn" | "warning" => Ok(Level::Warn),
            "error" => Ok(Level::Error),
            _ => Err(LogError::UnknownLevel(trimmed.to_string())),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error => 3,
        }
    }

    pub fn enables(self, level: Level) -> bool {
        level.rank() >= self.rank()
    }
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for Level {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for Level {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Level::parse(&raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_matches_go_aliases() {
        assert_eq!(Level::parse("debug"), Ok(Level::Debug));
        assert_eq!(Level::parse(""), Ok(Level::Info));
        assert_eq!(Level::parse(" INFO "), Ok(Level::Info));
        assert_eq!(Level::parse("warning"), Ok(Level::Warn));
        assert_eq!(Level::parse("ERROR"), Ok(Level::Error));
        assert!(Level::parse("trace").is_err());
    }

    #[test]
    fn unknown_level_message_is_chinese() {
        let error = Level::parse("trace").expect_err("trace 应被拒绝");
        assert_eq!(
            error.to_string(),
            "未知日志级别 \"trace\" (可选 debug/info/warn/error)",
            "错误信息需为中文原样文案"
        );
    }

    #[test]
    fn rank_orders_and_enables() {
        assert!(Level::Debug < Level::Info);
        assert!(Level::Warn < Level::Error);
        assert!(Level::Warn.enables(Level::Error), "warn 阈值应放行 error");
        assert!(!Level::Warn.enables(Level::Info), "warn 阈值应拦住 info");
        assert!(Level::Debug.enables(Level::Debug), "debug 阈值应放行 debug");
    }

    #[test]
    fn serde_uses_uppercase_label() {
        let encoded = serde_json::to_string(&Level::Warn).expect("序列化");
        assert_eq!(encoded, "\"WARN\"");
        let decoded: Level = serde_json::from_str("\"warn\"").expect("反序列化");
        assert_eq!(decoded, Level::Warn);
    }
}
