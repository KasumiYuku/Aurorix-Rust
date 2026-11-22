use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use aurorix_core::{RoleRequired, DEFAULT_DENIED_MESSAGE, DEFAULT_MAINTENANCE_MESSAGE};

pub const DENY_MESSAGE_LIMIT: usize = 300;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Maintenance {
    pub enabled: bool,
    pub message: String,
}

impl Maintenance {
    pub fn on(message: impl Into<String>) -> Self {
        Self {
            enabled: true,
            message: message.into(),
        }
    }

    pub fn normalized(&self) -> Self {
        let message = self.message.trim();
        if !self.enabled {
            return Self::default();
        }
        if message.is_empty() {
            return Self::on(DEFAULT_MAINTENANCE_MESSAGE);
        }
        Self::on(message)
    }

    pub fn text(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let message = self.message.trim();
        if message.is_empty() {
            Some(DEFAULT_MAINTENANCE_MESSAGE.to_string())
        } else {
            Some(message.to_string())
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deny {
    pub reply: Option<bool>,
    pub message: String,
}

impl Deny {
    pub fn silent() -> Self {
        Self::default()
    }

    pub fn say(message: impl Into<String>) -> Self {
        Self {
            reply: Some(true),
            message: message.into(),
        }
    }

    pub fn is_decided(&self) -> bool {
        self.reply.is_some()
    }

    pub fn normalized(&self) -> Self {
        let message = truncate_runes(self.message.trim(), DENY_MESSAGE_LIMIT);
        let mut reply = self.reply;
        if !message.is_empty() && reply.is_none() {
            reply = Some(true);
        }
        if reply == Some(false) {
            return Self {
                reply,
                message: String::new(),
            };
        }
        Self { reply, message }
    }
}

fn truncate_runes(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccessMode {
    #[default]
    Off,
    Whitelist,
    Blacklist,
}

impl AccessMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "off" => Some(Self::Off),
            "whitelist" => Some(Self::Whitelist),
            "blacklist" => Some(Self::Blacklist),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Whitelist => "whitelist",
            Self::Blacklist => "blacklist",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessRule {
    pub mode: AccessMode,
    pub users: Vec<String>,
    pub groups: Vec<String>,
    pub role: Option<RoleRequired>,
    pub maintenance: Maintenance,
    pub deny: Deny,
}

impl AccessRule {
    pub fn is_active(&self) -> bool {
        self.mode != AccessMode::Off || self.role.is_some() || self.maintenance.enabled
    }

    pub fn normalized(&self) -> Self {
        let mode = self.mode;
        Self {
            mode,
            users: clean_ids(&self.users),
            groups: clean_ids(&self.groups),
            role: self.role,
            maintenance: if self.maintenance.enabled {
                Maintenance::on(self.maintenance.message.trim())
            } else {
                Maintenance::default()
            },
            deny: if mode == AccessMode::Off {
                Deny::default()
            } else {
                self.deny.normalized()
            },
        }
    }
}

fn clean_ids(values: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || seen.iter().any(|item| item == trimmed) {
            continue;
        }
        seen.push(trimmed.to_string());
    }
    seen.sort();
    seen
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessConfig {
    pub default: AccessRule,
    pub commands: BTreeMap<String, AccessRule>,
    pub disabled: bool,
    pub maintenance: Maintenance,
    pub deny: Deny,
}

impl AccessConfig {
    pub fn plugin_maintenance(mut self, message: impl Into<String>) -> Self {
        self.maintenance = Maintenance::on(message);
        self
    }

    pub fn command_rule(mut self, path: &str, rule: AccessRule) -> Self {
        self.commands.insert(canonical_command_path(path), rule);
        self
    }

    pub fn normalized(&self) -> Self {
        let mut commands = BTreeMap::new();
        for (path, rule) in &self.commands {
            commands.insert(canonical_command_path(path), rule.normalized());
        }
        Self {
            default: AccessRule {
                maintenance: Maintenance::default(),
                ..self.default.normalized()
            },
            commands,
            disabled: self.disabled,
            maintenance: self.maintenance.normalized(),
            deny: self.deny.normalized(),
        }
    }
}

pub fn canonical_command_path(path: &str) -> String {
    path.split_whitespace()
        .map(crate::command::normalize_name)
        .collect::<Vec<String>>()
        .join(" ")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessQuery {
    pub plugin: String,
    pub command_path: String,
    pub user: String,
    pub conversation: String,
    pub role: Option<RoleRequired>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Disabled,
    Maintenance(String),
    CommandMaintenance(String),
    RoleDenied,
    Denied(String),
    DeniedSilent,
}

impl Verdict {
    pub fn allowed(&self) -> bool {
        matches!(self, Verdict::Allowed)
    }

    pub fn is_silent(&self) -> bool {
        matches!(
            self,
            Verdict::Disabled | Verdict::RoleDenied | Verdict::DeniedSilent
        )
    }
}

pub fn check_access(
    config: &AccessConfig,
    query: &AccessQuery,
    inline: Option<&AccessRule>,
) -> Verdict {
    if config.disabled {
        return Verdict::Disabled;
    }
    let exempt = RoleRequired::Admin.can_use(query.role);
    if config.maintenance.enabled && !exempt {
        let text = config.maintenance.text().unwrap_or_default();
        return Verdict::Maintenance(text);
    }
    let (rule, overridden) = match inline {
        Some(rule) => (rule.clone(), true),
        None => match config
            .commands
            .get(&canonical_command_path(&query.command_path))
        {
            Some(rule) => (rule.clone(), true),
            None => (config.default.clone(), false),
        },
    };
    if overridden && rule.maintenance.enabled && !exempt {
        return Verdict::CommandMaintenance(command_maintenance_message(
            &rule.maintenance,
            &config.maintenance,
        ));
    }
    if let Some(required) = rule.role {
        if !required.can_use(query.role) {
            return Verdict::RoleDenied;
        }
    }
    let user_matched = matched(&rule.users, &query.user);
    let group_matched = matched(&rule.groups, &query.conversation);
    let allowed = match rule.mode {
        AccessMode::Whitelist => user_matched || group_matched,
        AccessMode::Blacklist => !user_matched && !group_matched,
        AccessMode::Off => true,
    };
    if allowed {
        return Verdict::Allowed;
    }
    match deny_message(&rule.deny, &config.deny) {
        Some(text) => Verdict::Denied(text),
        None => Verdict::DeniedSilent,
    }
}

fn matched(values: &[String], target: &str) -> bool {
    if target.is_empty() {
        return false;
    }
    values.iter().any(|value| value == target)
}

pub fn command_maintenance_message(command: &Maintenance, plugin: &Maintenance) -> String {
    if let Some(text) = command.text() {
        return text;
    }
    if let Some(text) = plugin.text() {
        return text;
    }
    DEFAULT_MAINTENANCE_MESSAGE.to_string()
}

pub fn deny_message(command: &Deny, plugin: &Deny) -> Option<String> {
    let reply = match command.reply {
        Some(value) => value,
        None => plugin.reply.unwrap_or(false),
    };
    if !reply {
        return None;
    }
    let command_message = command.message.trim();
    if !command_message.is_empty() {
        return Some(command_message.to_string());
    }
    let plugin_message = plugin.message.trim();
    if !plugin_message.is_empty() {
        return Some(plugin_message.to_string());
    }
    Some(DEFAULT_DENIED_MESSAGE.to_string())
}

#[derive(Debug)]
pub struct AccessBook {
    state: ArcSwap<BTreeMap<String, AccessConfig>>,
    write_lock: Mutex<()>,
}

impl Default for AccessBook {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessBook {
    pub fn new() -> Self {
        Self {
            state: ArcSwap::from_pointee(BTreeMap::new()),
            write_lock: Mutex::new(()),
        }
    }

    pub fn set(&self, plugin: &str, config: AccessConfig) {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = BTreeMap::clone(self.state.load_full().as_ref());
        next.insert(plugin.to_string(), config.normalized());
        self.state.store(Arc::new(next));
    }

    pub fn get(&self, plugin: &str) -> Option<AccessConfig> {
        self.state.load_full().get(plugin).cloned()
    }

    pub fn disabled(&self, plugin: &str) -> bool {
        self.state
            .load_full()
            .get(plugin)
            .map(|config| config.disabled)
            .unwrap_or(false)
    }

    pub fn plugin_maintenance(&self, plugin: &str) -> Maintenance {
        self.state
            .load_full()
            .get(plugin)
            .map(|config| config.maintenance.clone())
            .unwrap_or_default()
    }

    pub fn check(&self, query: &AccessQuery, inline: Option<&AccessRule>) -> Verdict {
        let state = self.state.load_full();
        let Some(config) = state.get(&query.plugin) else {
            return Verdict::Allowed;
        };
        check_access(config, query, inline)
    }
}

pub type SharedAccessBook = Arc<AccessBook>;

#[cfg(test)]
mod tests {
    use super::*;

    fn query(role: Option<RoleRequired>) -> AccessQuery {
        AccessQuery {
            plugin: "demo".to_string(),
            command_path: "p song".to_string(),
            user: "u1".to_string(),
            conversation: "g1".to_string(),
            role,
        }
    }

    #[test]
    fn maintenance_normalization_fills_and_clears() {
        let empty = Maintenance {
            enabled: true,
            message: "   ".to_string(),
        };
        assert_eq!(
            empty.normalized().text(),
            Some(DEFAULT_MAINTENANCE_MESSAGE.to_string())
        );
        let off = Maintenance {
            enabled: false,
            message: "不该留下".to_string(),
        };
        assert_eq!(off.normalized(), Maintenance::default(), "关掉就清空文案");
        assert_eq!(off.text(), None);
        assert_eq!(Maintenance::default().text(), None);
    }

    #[test]
    fn deny_normalization_follows_the_three_rules() {
        let implied = Deny {
            reply: None,
            message: "  有文案就回话 ".to_string(),
        }
        .normalized();
        assert_eq!(implied.reply, Some(true));
        assert_eq!(implied.message, "有文案就回话");
        let silent = Deny {
            reply: Some(false),
            message: "关掉回话".to_string(),
        }
        .normalized();
        assert_eq!(silent.reply, Some(false));
        assert!(silent.message.is_empty(), "关掉回话要清空文案");
        let long = "字".repeat(DENY_MESSAGE_LIMIT + 50);
        let truncated = Deny {
            reply: Some(true),
            message: long,
        }
        .normalized();
        assert_eq!(truncated.message.chars().count(), DENY_MESSAGE_LIMIT);
        assert!(Deny::say("x").is_decided());
        assert!(!Deny::default().is_decided());
    }

    #[test]
    fn canonical_path_normalizes_every_segment() {
        assert_eq!(canonical_command_path(" /p   #song "), "p song");
        assert_eq!(canonical_command_path("a !b c"), "a b c");
    }

    #[test]
    fn access_mode_parses_with_case_and_blank() {
        assert_eq!(
            AccessMode::parse(" WHITELIST "),
            Some(AccessMode::Whitelist)
        );
        assert_eq!(AccessMode::parse("blacklist"), Some(AccessMode::Blacklist));
        assert_eq!(AccessMode::parse(""), Some(AccessMode::Off));
        assert_eq!(AccessMode::parse("nope"), None);
        assert_eq!(AccessMode::Blacklist.as_str(), "blacklist");
    }

    #[test]
    fn whitelist_allows_only_listed_user_or_group() {
        let rule = AccessRule {
            mode: AccessMode::Whitelist,
            users: vec!["u1".to_string()],
            groups: vec!["g9".to_string()],
            ..AccessRule::default()
        };
        let config = AccessConfig {
            default: rule,
            ..AccessConfig::default()
        };
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Member)), None),
            Verdict::Allowed
        );
        let mut other = query(Some(RoleRequired::Member));
        other.user = "u2".to_string();
        assert_eq!(check_access(&config, &other, None), Verdict::DeniedSilent);
        let mut member = query(Some(RoleRequired::Member));
        member.user = "u2".to_string();
        member.conversation = "g9".to_string();
        assert_eq!(
            check_access(&config, &member, None),
            Verdict::Allowed,
            "命中群也算白名单"
        );
    }

    #[test]
    fn blacklist_blocks_listed_user() {
        let config = AccessConfig {
            default: AccessRule {
                mode: AccessMode::Blacklist,
                users: vec!["u1".to_string()],
                deny: Deny::say("黑名单"),
                ..AccessRule::default()
            },
            ..AccessConfig::default()
        };
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Member)), None),
            Verdict::Denied("黑名单".to_string())
        );
        let mut other = query(Some(RoleRequired::Member));
        other.user = "u2".to_string();
        assert_eq!(
            check_access(&config, &other, None),
            Verdict::Allowed,
            "不在黑名单里的用户要放行"
        );
    }

    #[test]
    fn empty_user_never_matches_lists() {
        let config = AccessConfig {
            default: AccessRule {
                mode: AccessMode::Blacklist,
                users: vec![String::new()],
                ..AccessRule::default()
            },
            ..AccessConfig::default()
        };
        let mut empty = query(Some(RoleRequired::Member));
        empty.user.clear();
        assert_eq!(
            check_access(&config, &empty, None),
            Verdict::Allowed,
            "空身份不该被名单命中"
        );
    }

    #[test]
    fn plugin_maintenance_exempts_admin() {
        let config = AccessConfig::default().plugin_maintenance("插件维护中");
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Member)), None),
            Verdict::Maintenance("插件维护中".to_string())
        );
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Admin)), None),
            Verdict::Allowed
        );
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Owner)), None),
            Verdict::Allowed
        );
    }

    #[test]
    fn disabled_plugin_reports_disabled_first() {
        let config = AccessConfig {
            disabled: true,
            ..AccessConfig::default().plugin_maintenance("维护中")
        };
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Owner)), None),
            Verdict::Disabled
        );
        assert!(Verdict::Disabled.is_silent());
        assert!(!Verdict::Allowed.is_silent());
    }

    #[test]
    fn command_rule_overrides_the_default() {
        let config = AccessConfig::default().command_rule(
            "/p #song",
            AccessRule {
                maintenance: Maintenance::on("指令维护中"),
                ..AccessRule::default()
            },
        );
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Member)), None),
            Verdict::CommandMaintenance("指令维护中".to_string()),
            "指令级维护要命中规范路径"
        );
        let inline = AccessRule {
            role: Some(RoleRequired::Owner),
            ..AccessRule::default()
        };
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Member)), Some(&inline)),
            Verdict::RoleDenied,
            "内联规则优先"
        );
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Owner)), Some(&inline)),
            Verdict::Allowed
        );
    }

    #[test]
    fn rule_role_denies_silently() {
        let config = AccessConfig {
            default: AccessRule {
                role: Some(RoleRequired::Admin),
                ..AccessRule::default()
            },
            ..AccessConfig::default()
        };
        assert_eq!(
            check_access(&config, &query(Some(RoleRequired::Member)), None),
            Verdict::RoleDenied
        );
        assert!(Verdict::RoleDenied.is_silent());
    }

    #[test]
    fn normalized_config_clears_default_maintenance() {
        let config = AccessConfig {
            default: AccessRule {
                maintenance: Maintenance::on("不该留"),
                ..AccessRule::default()
            },
            maintenance: Maintenance::on("  "),
            deny: Deny {
                reply: None,
                message: "名单话术".to_string(),
            },
            ..AccessConfig::default()
        }
        .normalized();
        assert!(
            !config.default.maintenance.enabled,
            "默认规则不许挂维护开关"
        );
        assert_eq!(
            config.maintenance.text(),
            Some(DEFAULT_MAINTENANCE_MESSAGE.to_string())
        );
        assert_eq!(config.deny.reply, Some(true));
    }

    #[test]
    fn access_rule_is_active_reports_real_constraints() {
        assert!(!AccessRule::default().is_active());
        assert!(AccessRule {
            role: Some(RoleRequired::Member),
            ..AccessRule::default()
        }
        .is_active());
        assert!(AccessRule {
            maintenance: Maintenance::on("x"),
            ..AccessRule::default()
        }
        .is_active());
        assert!(AccessRule {
            mode: AccessMode::Whitelist,
            ..AccessRule::default()
        }
        .is_active());
    }

    #[test]
    fn book_defaults_to_allowed_and_can_store_configs() {
        let book = AccessBook::new();
        assert_eq!(
            book.check(&query(Some(RoleRequired::Member)), None),
            Verdict::Allowed,
            "没配置过就是全放行"
        );
        assert!(!book.disabled("demo"));
        assert!(book.plugin_maintenance("demo").text().is_none());
        book.set("demo", AccessConfig::default().plugin_maintenance("维护中"));
        assert_eq!(
            book.check(&query(Some(RoleRequired::Member)), None),
            Verdict::Maintenance("维护中".to_string())
        );
        assert!(book.plugin_maintenance("demo").text().is_some());
        book.set(
            "demo",
            AccessConfig {
                disabled: true,
                ..AccessConfig::default()
            },
        );
        assert!(book.disabled("demo"));
        assert!(book.get("demo").is_some());
        assert!(book.get("missing").is_none());
    }

    #[test]
    fn maintenance_message_falls_back_through_layers() {
        assert_eq!(
            command_maintenance_message(&Maintenance::on("指令"), &Maintenance::on("插件")),
            "指令"
        );
        assert_eq!(
            command_maintenance_message(&Maintenance::default(), &Maintenance::on("插件")),
            "插件"
        );
        assert_eq!(
            command_maintenance_message(&Maintenance::default(), &Maintenance::default()),
            DEFAULT_MAINTENANCE_MESSAGE
        );
    }

    #[test]
    fn deny_message_falls_back_through_layers() {
        assert_eq!(deny_message(&Deny::default(), &Deny::default()), None);
        assert_eq!(
            deny_message(&Deny::default(), &Deny::say("插件话术")).as_deref(),
            Some("插件话术")
        );
        assert_eq!(
            deny_message(&Deny::say("指令话术"), &Deny::say("插件话术")).as_deref(),
            Some("指令话术")
        );
        assert_eq!(
            deny_message(&Deny::say("   "), &Deny::say("插件话术")).as_deref(),
            Some("插件话术")
        );
        assert_eq!(
            deny_message(
                &Deny {
                    reply: Some(true),
                    message: String::new()
                },
                &Deny::default()
            )
            .as_deref(),
            Some(DEFAULT_DENIED_MESSAGE)
        );
        assert_eq!(
            deny_message(
                &Deny {
                    reply: Some(false),
                    message: "关掉".to_string()
                },
                &Deny::say("插件话术")
            ),
            None,
            "指令显式静默要盖过插件级"
        );
    }

    #[test]
    fn clean_ids_trims_dedups_and_sorts() {
        let rule = AccessRule {
            users: vec![
                " b ".to_string(),
                "a".to_string(),
                "a".to_string(),
                "  ".to_string(),
            ],
            ..AccessRule::default()
        }
        .normalized();
        assert_eq!(rule.users, vec!["a".to_string(), "b".to_string()]);
    }
}
