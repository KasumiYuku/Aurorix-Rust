use std::sync::{Arc, RwLock};

use aurorix_core::{RoleRequired, Scene, DEFAULT_DENIED_MESSAGE, DEFAULT_MAINTENANCE_MESSAGE};
use aurorix_template::Registry;

use crate::access::{AccessBook, AccessQuery, Deny, Maintenance, Verdict};
use crate::args::{parse_args, ParsedArgs};
use crate::assets::PluginAssets;
use crate::ban::BanList;
use crate::card;
use crate::command::CommandNode;
use crate::error::PluginError;
use crate::rate::{RateKey, RateLimiter};
use crate::registry::CommandRegistry;
use crate::reply::{Incoming, Invocation, InvocationSender, Outgoing, PushSink, ReplyKind, ReplySink};
use crate::roles::{PlatformRoleResolver, RoleResolver};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    GlobalMaintenance = 1,
    ResolveRoot,
    PluginDisabled,
    Banned,
    CommandRole,
    PrivateBlocked,
    PluginMaintenance,
    CommandMaintenance,
    RuleRole,
    AccessList,
    HandlerMissing,
    ArgumentError,
    Cooldown,
    Handled,
}

impl Stage {
    pub const fn level(self) -> u8 {
        self as u8
    }

    pub const fn label(self) -> &'static str {
        match self {
            Stage::GlobalMaintenance => "全局维护",
            Stage::ResolveRoot => "指令解析",
            Stage::PluginDisabled => "插件停用",
            Stage::Banned => "封禁",
            Stage::CommandRole => "指令权限",
            Stage::PrivateBlocked => "私聊禁用",
            Stage::PluginMaintenance => "插件维护",
            Stage::CommandMaintenance => "指令维护",
            Stage::RuleRole => "名单权限",
            Stage::AccessList => "名单过滤",
            Stage::HandlerMissing => "处理器缺失",
            Stage::ArgumentError => "参数错误",
            Stage::Cooldown => "冷却",
            Stage::Handled => "已执行",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchOutcome {
    pub stage: Stage,
    pub invocation: Invocation,
    pub reply: Option<Outgoing>,
}

impl DispatchOutcome {
    pub fn silent(&self) -> bool {
        self.reply.is_none()
    }

    pub fn text(&self) -> Option<String> {
        self.reply.as_ref().map(|reply| reply.body())
    }
}

#[derive(Debug, Clone, Default)]
pub struct PluginAssetBook {
    entries: std::collections::BTreeMap<String, Arc<PluginAssets>>,
}

impl PluginAssetBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, plugin_id: impl Into<String>, assets: Arc<PluginAssets>) -> Self {
        self.entries.insert(plugin_id.into(), assets);
        self
    }

    pub fn insert(&mut self, plugin_id: impl Into<String>, assets: Arc<PluginAssets>) {
        self.entries.insert(plugin_id.into(), assets);
    }

    pub fn get(&self, plugin_id: &str) -> Option<Arc<PluginAssets>> {
        self.entries.get(plugin_id).map(Arc::clone)
    }

    pub fn contains(&self, plugin_id: &str) -> bool {
        self.entries.contains_key(plugin_id)
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct Router {
    registry: Arc<CommandRegistry>,
    access: Arc<AccessBook>,
    ban: Arc<BanList>,
    roles: Arc<dyn RoleResolver>,
    limiter: Arc<RateLimiter>,
    templates: Arc<Registry>,
    maintenance: RwLock<Maintenance>,
    plugin_dbs: Option<Arc<aurorix_db::PluginDbRegistry>>,
    plugin_assets: Option<Arc<PluginAssetBook>>,
    push: RwLock<Option<PushSink>>,
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Router")
            .field("commands", &self.registry.command_count())
            .finish()
    }
}

impl Router {
    pub fn new(registry: Arc<CommandRegistry>) -> Self {
        Self {
            registry,
            access: Arc::new(AccessBook::new()),
            ban: Arc::new(BanList::new()),
            roles: Arc::new(PlatformRoleResolver),
            limiter: Arc::new(RateLimiter::new()),
            templates: Arc::new(Registry::new()),
            maintenance: RwLock::new(Maintenance::default()),
            plugin_dbs: None,
            plugin_assets: None,
            push: RwLock::new(None),
        }
    }

    pub fn with_access(mut self, access: Arc<AccessBook>) -> Self {
        self.access = access;
        self
    }

    pub fn with_ban(mut self, ban: Arc<BanList>) -> Self {
        self.ban = ban;
        self
    }

    pub fn with_roles(mut self, roles: Arc<dyn RoleResolver>) -> Self {
        self.roles = roles;
        self
    }

    pub fn with_limiter(mut self, limiter: Arc<RateLimiter>) -> Self {
        self.limiter = limiter;
        self
    }

    pub fn with_templates(mut self, templates: Arc<Registry>) -> Self {
        self.templates = templates;
        self
    }
    pub fn with_plugin_db_registry(mut self, registry: Arc<aurorix_db::PluginDbRegistry>) -> Self {
        self.plugin_dbs = Some(registry);
        self
    }

    pub fn with_plugin_assets(mut self, book: Arc<PluginAssetBook>) -> Self {
        self.plugin_assets = Some(book);
        self
    }

    pub fn registry(&self) -> &Arc<CommandRegistry> {
        &self.registry
    }

    pub fn access(&self) -> &Arc<AccessBook> {
        &self.access
    }

    pub fn ban(&self) -> &Arc<BanList> {
        &self.ban
    }

    pub fn limiter(&self) -> &Arc<RateLimiter> {
        &self.limiter
    }

    pub fn set_global_maintenance(&self, maintenance: Maintenance) {
        let mut guard = self
            .maintenance
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = maintenance.normalized();
    }

    pub fn set_push_sink(&self, sink: PushSink) {
        let mut guard = self
            .push
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(sink);
    }

    pub fn push_sink(&self) -> Option<PushSink> {
        let guard = self
            .push
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }

    pub fn global_maintenance(&self) -> Maintenance {
        let guard = self
            .maintenance
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }

    pub async fn dispatch(&self, event: Incoming) -> Option<Outgoing> {
        self.dispatch_detailed(event).await.reply
    }

    pub async fn dispatch_with<S: ReplySink + ?Sized>(
        &self,
        sink: &S,
        event: Incoming,
    ) -> Result<Option<Outgoing>, PluginError> {
        let outcome = self.dispatch_detailed(event).await;
        if let Some(reply) = &outcome.reply {
            sink.reply(&outcome.invocation, reply.clone()).await?;
        }
        Ok(outcome.reply)
    }

    pub async fn dispatch_detailed(&self, event: Incoming) -> DispatchOutcome {
        self.dispatch_detailed_with(event, None).await
    }

    pub async fn dispatch_detailed_with(
        &self,
        event: Incoming,
        sender: Option<InvocationSender>,
    ) -> DispatchOutcome {
        let mut invocation = Invocation::from_event(&event);
        if let Some(sender) = sender {
            invocation = invocation.with_sender(sender);
        }
        if let Some(sink) = self.push_sink() {
            invocation = invocation.with_push(sink);
        }
        let tokens = split_fields(&event.content);
        let Some(root) = self.registry.resolve_root(&tokens) else {
            return outcome(Stage::ResolveRoot, invocation, None);
        };
        let role = self
            .roles
            .resolve(&event.user, &event.target.id, event.platform_role);
        let privileged = RoleRequired::Admin.can_use(role);
        if !privileged {
            if let Some(text) = self.global_maintenance().text() {
                return outcome(
                    Stage::GlobalMaintenance,
                    invocation,
                    Some(maintenance_reply(&text, &event)),
                );
            }
        }
        let root_node = Arc::clone(&root.node);
        if !privileged && self.access.disabled(&root_node.plugin) {
            return outcome(Stage::PluginDisabled, invocation, None);
        }
        if let Some(text) = self.ban.hit(&event.user) {
            return outcome(Stage::Banned, invocation, Some(banned_reply(&text, &event)));
        }
        let (node, path, consumed) = self.registry.resolve(Arc::clone(&root_node), &root.rest);
        invocation.plugin = node.plugin.clone();
        if let Some(registry) = &self.plugin_dbs {
            if let Ok(handle) = registry.plugin(&node.plugin) {
                invocation = invocation.with_plugin_db(handle);
            }
        }
        if let Some(book) = &self.plugin_assets {
            if let Some(assets) = book.get(&node.plugin) {
                invocation = invocation.with_plugin_assets(assets);
            }
        }
        invocation.command = path.clone();
        invocation.no_footer = root_node.no_footer || node.no_footer;
        invocation.role = role;
        if !role_allows(root_node.role, role) {
            return outcome(
                Stage::CommandRole,
                invocation,
                deny_reply(&root_node.deny, &event),
            );
        }
        if !Arc::ptr_eq(&root_node, &node) && !role_allows(node.role, role) {
            return outcome(
                Stage::CommandRole,
                invocation,
                deny_reply(&node.deny, &event),
            );
        }
        if event.private && (root_node.disable_private || node.disable_private) {
            return outcome(
                Stage::PrivateBlocked,
                invocation,
                deny_reply(&node.deny, &event),
            );
        }
        let query = AccessQuery {
            plugin: node.plugin.clone(),
            command_path: path.clone(),
            user: event.user.clone(),
            conversation: event.target.id.clone(),
            role,
        };
        let verdict = match privileged {
            true => Verdict::Allowed,
            false => self.access.check(&query, node.access.as_ref()),
        };
        match verdict {
            Verdict::Allowed => {}
            Verdict::Disabled => return outcome(Stage::PluginDisabled, invocation, None),
            Verdict::Maintenance(text) => {
                return outcome(
                    Stage::PluginMaintenance,
                    invocation,
                    Some(maintenance_reply(&text, &event)),
                )
            }
            Verdict::CommandMaintenance(text) => {
                return outcome(
                    Stage::CommandMaintenance,
                    invocation,
                    Some(maintenance_reply(&text, &event)),
                )
            }
            Verdict::RoleDenied => return outcome(Stage::RuleRole, invocation, None),
            Verdict::Denied(text) => {
                return outcome(
                    Stage::AccessList,
                    invocation,
                    Some(denied_reply(&text, &event)),
                )
            }
            Verdict::DeniedSilent => return outcome(Stage::AccessList, invocation, None),
        }
        let root_like = Arc::ptr_eq(&root_node, &node);
        let handler = if root_like && root_node.fallback.is_some() {
            match root.rest.is_empty() {
                true => node.handler.clone().or_else(|| root_node.fallback.clone()),
                false => root_node.fallback.clone(),
            }
        } else {
            node.handler.clone()
        };
        let Some(handler) = handler else {
            return outcome(Stage::HandlerMissing, invocation, None);
        };
        let rest = &root.rest[consumed..];
        let text = raw_args(
            &event.content,
            &tokens,
            rest,
            &root_node,
            &self.registry.prefix_chars(),
        );
        match node.args.clone() {
            Some(spec) => match parse_args(&path, &spec, &text) {
                Ok(parsed) => invocation.args = parsed,
                Err(error) => {
                    let usage = if error.usage.trim().is_empty() {
                        usage_text(&self.registry, &path)
                    } else {
                        error.usage
                    };
                    let mut reply =
                        card::argument_error(&self.templates, &node.plugin, &error.message, &usage);
                    reply.at = mentionable(&event);
                    return outcome(Stage::ArgumentError, invocation, Some(reply));
                }
            },
            None => invocation.args = ParsedArgs::from_tokens(rest),
        }
        invocation.text = text;
        let key = RateKey::new(path.clone(), event.target.id.clone(), event.user.clone());
        if let Err(wait) = self.limiter.check(key, node.cooldown) {
            return outcome(Stage::Cooldown, invocation, Some(rate_reply(wait, &event)));
        }
        let mut reply = match handler.handle(invocation.clone()).await {
            Ok(reply) => reply,
            Err(error) => {
                let mut reply =
                    card::handler_failure(&self.templates, &node.plugin, &error.to_string());
                reply.at = mentionable(&event);
                Some(reply)
            }
        };
        if let Some(out) = reply.as_mut() {
            out.stamp_plugin(&invocation.plugin);
        }
        outcome(Stage::Handled, invocation, reply)
    }
}

fn outcome(stage: Stage, invocation: Invocation, reply: Option<Outgoing>) -> DispatchOutcome {
    DispatchOutcome {
        stage,
        invocation,
        reply,
    }
}

pub fn split_fields(content: &str) -> Vec<String> {
    content.split_whitespace().map(str::to_string).collect()
}

pub fn role_allows(required: Option<RoleRequired>, role: Option<RoleRequired>) -> bool {
    match required {
        None => true,
        Some(required) => required.can_use(role),
    }
}

pub fn usage_text(registry: &CommandRegistry, path: &str) -> String {
    format!("{}{path}", registry.display_prefix())
}

fn mentionable(event: &Incoming) -> bool {
    !event.target.id.is_empty() && !event.target.user.is_empty()
}

fn maintenance_reply(text: &str, event: &Incoming) -> Outgoing {
    let mut reply = Outgoing::new(ReplyKind::Maintenance, text).no_footer();
    reply.at = mentionable(event);
    reply
}

fn denied_reply(text: &str, event: &Incoming) -> Outgoing {
    let mut reply = Outgoing::new(ReplyKind::Denied, text).no_footer();
    reply.at = mentionable(event);
    reply
}

fn banned_reply(text: &str, event: &Incoming) -> Outgoing {
    let mut reply = Outgoing::new(ReplyKind::Banned, text).no_footer();
    reply.at = mentionable(event) && event.scene != Scene::C2c;
    reply
}

fn rate_reply(wait: std::time::Duration, event: &Incoming) -> Outgoing {
    let error = PluginError::RateLimited {
        seconds: RateLimiter::wait_seconds(wait),
    };
    let mut reply = Outgoing::new(ReplyKind::RateLimited, error.to_string()).no_footer();
    reply.at = mentionable(event);
    reply
}

fn deny_reply(deny: &Deny, event: &Incoming) -> Option<Outgoing> {
    if deny.reply != Some(true) {
        return None;
    }
    let message = deny.message.trim();
    let text = if message.is_empty() {
        DEFAULT_DENIED_MESSAGE
    } else {
        message
    };
    Some(denied_reply(text, event))
}

pub fn default_maintenance_text() -> &'static str {
    DEFAULT_MAINTENANCE_MESSAGE
}

pub fn raw_args(
    content: &str,
    tokens: &[String],
    rest: &[String],
    root: &CommandNode,
    prefixes: &[String],
) -> String {
    let consumed = tokens.len().saturating_sub(rest.len());
    if consumed > 0 {
        let mut position = 0usize;
        for token in &tokens[..consumed] {
            match content[position.min(content.len())..].find(token.as_str()) {
                Some(offset) => position += offset + token.len(),
                None => return rest.join(" "),
            }
            if position > content.len() {
                return rest.join(" ");
            }
        }
        return content[position..]
            .trim_start_matches([' ', '\t', '\n'])
            .to_string();
    }
    for prefix in prefixes {
        if prefix.is_empty() || !tokens[0].starts_with(prefix.as_str()) {
            continue;
        }
        let Some(offset) = content.find(tokens[0].as_str()) else {
            return rest.join(" ");
        };
        let start = offset + prefix.len() + root.name.len();
        if start > content.len() || !content.is_char_boundary(start) {
            return rest.join(" ");
        }
        return content[start..].to_string();
    }
    rest.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurorix_core::Target;

    fn group() -> Incoming {
        Incoming::new(
            Scene::Group,
            Target::new(Scene::Group, "g1").with_user("u1"),
            "u1",
            "/echo hi",
        )
    }

    #[test]
    fn stage_levels_follow_the_go_table() {
        assert_eq!(Stage::GlobalMaintenance.level(), 1);
        assert_eq!(Stage::ResolveRoot.level(), 2);
        assert_eq!(Stage::PluginDisabled.level(), 3);
        assert_eq!(Stage::Banned.level(), 4);
        assert_eq!(Stage::CommandRole.level(), 5);
        assert_eq!(Stage::PrivateBlocked.level(), 6);
        assert_eq!(Stage::PluginMaintenance.level(), 7);
        assert_eq!(Stage::CommandMaintenance.level(), 8);
        assert_eq!(Stage::RuleRole.level(), 9);
        assert_eq!(Stage::AccessList.level(), 10);
        assert_eq!(Stage::HandlerMissing.level(), 11);
        assert_eq!(Stage::ArgumentError.level(), 12);
        assert_eq!(Stage::Cooldown.level(), 13);
        assert_eq!(Stage::Handled.level(), 14);
        assert_eq!(Stage::Banned.label(), "封禁");
    }

    #[test]
    fn split_fields_collapses_whitespace() {
        assert_eq!(
            split_fields("  a\t b\nc  "),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(split_fields("   ").is_empty());
    }

    #[test]
    fn role_allows_follows_core_rules() {
        assert!(role_allows(None, None), "不要求角色时谁都能用");
        assert!(role_allows(
            Some(RoleRequired::Member),
            Some(RoleRequired::Member)
        ));
        assert!(
            !role_allows(Some(RoleRequired::Member), None),
            "未知身份要拦"
        );
        assert!(!role_allows(
            Some(RoleRequired::Owner),
            Some(RoleRequired::Admin)
        ));
    }

    #[test]
    fn replies_carry_the_right_flags() {
        let event = group();
        let maintenance = maintenance_reply(DEFAULT_MAINTENANCE_MESSAGE, &event);
        assert!(maintenance.at, "群聊要艾特发起人");
        assert!(maintenance.no_footer, "维护话术不带底部按钮");
        assert_eq!(maintenance.kind, ReplyKind::Maintenance);
        let rate = rate_reply(std::time::Duration::from_secs(5), &event);
        assert_eq!(rate.body(), "⏳ 触发太频繁了，5 秒后再试");
        assert_eq!(rate.kind, ReplyKind::RateLimited);
        let c2c = Incoming::new(Scene::C2c, Target::new(Scene::C2c, "u1"), "u1", "x");
        assert!(!banned_reply("封了", &c2c).at, "私聊不艾特");
        assert!(banned_reply("封了", &event).at);
        assert!(!denied_reply("没权限", &c2c).at);
    }

    #[test]
    fn deny_reply_is_silent_by_default() {
        let event = group();
        assert!(deny_reply(&Deny::default(), &event).is_none());
        assert!(deny_reply(&Deny::say("自定义"), &event).is_some());
        let message = deny_reply(&Deny::say("   "), &event).expect("表态了就要回");
        assert_eq!(message.body(), DEFAULT_DENIED_MESSAGE);
        let silent = deny_reply(
            &Deny {
                reply: Some(false),
                message: "不该出现".to_string(),
            },
            &event,
        );
        assert!(silent.is_none(), "显式静默优先");
    }

    #[test]
    fn mention_needs_both_ids() {
        let mut event = group();
        assert!(mentionable(&event));
        event.target.user.clear();
        assert!(!mentionable(&event));
    }
}
