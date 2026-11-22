use std::collections::BTreeMap;
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;

use aurorix_core::RoleRequired;
use aurorix_sdk::Plugin;

use crate::access::{AccessRule, Deny, Maintenance};
use crate::args::ArgsSpec;
use crate::config_field::PluginConfigField;
use crate::error::PluginError;
use crate::rate::Rate;
use crate::reply::{Invocation, Outgoing};

pub const PREFIX_SYMBOLS: [&str; 3] = ["/", "#", "!"];

pub fn normalize_name(name: &str) -> String {
    for symbol in PREFIX_SYMBOLS {
        if let Some(rest) = name.strip_prefix(symbol) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    name.to_string()
}

pub fn prefix_symbol_of(token: &str, prefixes: &[String]) -> Option<String> {
    prefixes
        .iter()
        .find(|prefix| {
            !prefix.is_empty() && token.starts_with(prefix.as_str()) && token.len() > prefix.len()
        })
        .cloned()
}

pub type HandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<Outgoing>, PluginError>> + Send + 'a>>;

pub trait CommandHandler: Send + Sync {
    fn handle<'a>(&'a self, invocation: Invocation) -> HandlerFuture<'a>;
}

struct AsyncHandler<F> {
    inner: F,
}

impl<F, Fut> CommandHandler for AsyncHandler<F>
where
    F: Fn(Invocation) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Option<Outgoing>, PluginError>> + Send + 'static,
{
    fn handle<'a>(&'a self, invocation: Invocation) -> HandlerFuture<'a> {
        Box::pin((self.inner)(invocation))
    }
}

struct SyncHandler<F> {
    inner: F,
}

impl<F> CommandHandler for SyncHandler<F>
where
    F: Fn(&Invocation) -> Result<Option<Outgoing>, PluginError> + Send + Sync,
{
    fn handle<'a>(&'a self, invocation: Invocation) -> HandlerFuture<'a> {
        let reply = (self.inner)(&invocation);
        Box::pin(ready(reply))
    }
}

pub fn async_handler<F, Fut>(handler: F) -> Arc<dyn CommandHandler>
where
    F: Fn(Invocation) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Option<Outgoing>, PluginError>> + Send + 'static,
{
    Arc::new(AsyncHandler { inner: handler })
}

pub fn sync_handler<F>(handler: F) -> Arc<dyn CommandHandler>
where
    F: Fn(&Invocation) -> Result<Option<Outgoing>, PluginError> + Send + Sync + 'static,
{
    Arc::new(SyncHandler { inner: handler })
}

#[derive(Clone, Default)]
pub struct CommandSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub prefixes: Vec<String>,
    pub role: Option<RoleRequired>,
    pub cooldown: Rate,
    pub access: Option<AccessRule>,
    pub maintenance: Option<Maintenance>,
    pub deny: Option<Deny>,
    pub help: Option<String>,
    pub args: Option<ArgsSpec>,
    pub handler: Option<Arc<dyn CommandHandler>>,
    pub disable_private: bool,
    pub no_footer: bool,
    pub subcommands: Vec<CommandSpec>,
    pub fallback: Option<Arc<dyn CommandHandler>>,
}

impl CommandSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn from_macro(meta: (&'static str, bool), handler: Arc<dyn CommandHandler>) -> Self {
        let mut spec = Self::new(meta.0).handler(handler);
        if meta.1 {
            spec = spec.role(RoleRequired::Admin);
        }
        spec
    }

    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn aliases(mut self, aliases: impl IntoIterator<Item = String>) -> Self {
        self.aliases.extend(aliases);
        self
    }

    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefixes.push(prefix.into());
        self
    }

    pub fn prefixes(mut self, prefixes: impl IntoIterator<Item = String>) -> Self {
        self.prefixes.extend(prefixes);
        self
    }

    pub fn role(mut self, role: RoleRequired) -> Self {
        self.role = Some(role);
        self
    }

    pub fn cooldown(mut self, rate: Rate) -> Self {
        self.cooldown = rate;
        self
    }

    pub fn access(mut self, rule: AccessRule) -> Self {
        self.access = Some(rule);
        self
    }

    pub fn maintenance(mut self, maintenance: Maintenance) -> Self {
        self.maintenance = Some(maintenance);
        self
    }

    pub fn deny(mut self, deny: Deny) -> Self {
        self.deny = Some(deny);
        self
    }

    pub fn describe(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn handler(mut self, handler: Arc<dyn CommandHandler>) -> Self {
        self.handler = Some(handler);
        self
    }

    pub fn on<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(Invocation) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<Outgoing>, PluginError>> + Send + 'static,
    {
        self.handler = Some(async_handler(handler));
        self
    }

    pub fn on_sync<F>(mut self, handler: F) -> Self
    where
        F: Fn(&Invocation) -> Result<Option<Outgoing>, PluginError> + Send + Sync + 'static,
    {
        self.handler = Some(sync_handler(handler));
        self
    }

    pub fn disable_private(mut self) -> Self {
        self.disable_private = true;
        self
    }

    pub fn no_footer(mut self) -> Self {
        self.no_footer = true;
        self
    }

    pub fn with_args(mut self, args: ArgsSpec) -> Self {
        self.args = Some(args);
        self
    }

    pub fn subcommand(mut self, subcommand: CommandSpec) -> Self {
        self.subcommands.push(subcommand);
        self
    }

    pub fn subcommands(mut self, subcommands: impl IntoIterator<Item = CommandSpec>) -> Self {
        self.subcommands.extend(subcommands);
        self
    }

    pub fn fallback(mut self, handler: Arc<dyn CommandHandler>) -> Self {
        self.fallback = Some(handler);
        self
    }
}

pub struct CommandNode {
    pub name: String,
    pub path: String,
    pub aliases: Vec<String>,
    pub plugin: String,
    pub role: Option<RoleRequired>,
    pub disable_private: bool,
    pub no_footer: bool,
    pub cooldown: Rate,
    pub args: Option<ArgsSpec>,
    pub help: String,
    pub deny: Deny,
    pub maintenance: Option<Maintenance>,
    pub access: Option<AccessRule>,
    pub prefixes: Vec<String>,
    pub handler: Option<Arc<dyn CommandHandler>>,
    pub fallback: Option<Arc<dyn CommandHandler>>,
    pub children: BTreeMap<String, Arc<CommandNode>>,
    pub subcommands: Vec<Arc<CommandNode>>,
}

impl std::fmt::Debug for CommandNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandNode")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("plugin", &self.plugin)
            .field("aliases", &self.aliases)
            .finish()
    }
}

impl CommandNode {
    pub fn has_handler(&self) -> bool {
        self.handler.is_some()
    }

    pub fn accepts_prefix(&self, prefix: &str) -> bool {
        self.prefixes.is_empty() || self.prefixes.iter().any(|item| item == prefix)
    }

    pub fn child(&self, token: &str) -> Option<Arc<CommandNode>> {
        self.children.get(token).cloned()
    }

    pub fn canonical_path(&self) -> &str {
        &self.path
    }
}

pub fn build_node(spec: CommandSpec, plugin: &str, parent_path: &str) -> Arc<CommandNode> {
    let name = normalize_name(&spec.name);
    let path = if parent_path.is_empty() {
        name.clone()
    } else {
        format!("{parent_path} {name}")
    };
    let mut children = BTreeMap::new();
    let mut subcommands = Vec::new();
    for subcommand in spec.subcommands {
        let node = build_node(subcommand, plugin, &path);
        subcommands.push(Arc::clone(&node));
        children.insert(node.name.clone(), Arc::clone(&node));
        for alias in &node.aliases {
            children.insert(normalize_name(alias), Arc::clone(&node));
        }
    }
    Arc::new(CommandNode {
        name,
        path,
        aliases: spec.aliases,
        plugin: plugin.to_string(),
        role: spec.role,
        disable_private: spec.disable_private,
        no_footer: spec.no_footer,
        cooldown: spec.cooldown,
        args: spec.args,
        help: spec.help.unwrap_or_default(),
        deny: spec.deny.unwrap_or_default().normalized(),
        maintenance: spec.maintenance.map(|item| item.normalized()),
        access: spec.access.map(|rule| rule.normalized()),
        prefixes: spec.prefixes,
        handler: spec.handler,
        fallback: spec.fallback,
        children,
        subcommands,
    })
}

#[derive(Clone)]
pub struct PluginCommands {
    pub id: String,
    pub name: String,
    pub description: String,
    pub commands: Vec<CommandSpec>,
    pub config: Vec<PluginConfigField>,
}

impl PluginCommands {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            commands: Vec::new(),
            config: Vec::new(),
        }
    }

    pub fn from_plugin(plugin: &dyn Plugin, commands: Vec<CommandSpec>) -> Self {
        let meta = plugin.meta();
        Self {
            id: meta.id,
            name: meta.name,
            description: plugin.describe(),
            commands,
            config: Vec::new(),
        }
    }

    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn config(mut self, field: PluginConfigField) -> Self {
        self.config.push(field);
        self
    }

    pub fn with_config(mut self, fields: impl IntoIterator<Item = PluginConfigField>) -> Self {
        self.config.extend(fields);
        self
    }

    pub fn command(mut self, command: CommandSpec) -> Self {
        self.commands.push(command);
        self
    }

    pub fn with_commands(mut self, commands: impl IntoIterator<Item = CommandSpec>) -> Self {
        self.commands.extend(commands);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_one_symbol() {
        assert_eq!(normalize_name("/echo"), "echo");
        assert_eq!(normalize_name("#echo"), "echo");
        assert_eq!(normalize_name("!echo"), "echo");
        assert_eq!(normalize_name("//echo"), "/echo");
        assert_eq!(normalize_name("/"), "/");
        assert_eq!(normalize_name("echo"), "echo");
    }

    #[test]
    fn node_paths_join_with_space() {
        let spec = CommandSpec::new("/p").subcommand(CommandSpec::new("/song"));
        let node = build_node(spec, "arcaea", "");
        assert_eq!(node.path, "p");
        let child = node.child("song").expect("子指令要能按名字取到");
        assert_eq!(child.path, "p song");
        assert_eq!(child.plugin, "arcaea");
        assert_eq!(node.subcommands.len(), 1);
    }

    #[test]
    fn alias_keys_share_the_same_node() {
        let spec = CommandSpec::new("p").subcommand(CommandSpec::new("song").alias("歌"));
        let node = build_node(spec, "arcaea", "");
        assert!(node.child("song").is_some());
        let by_alias = node.child("歌").expect("别名要能取到子指令");
        assert_eq!(by_alias.path, "p song");
    }

    #[test]
    fn prefix_gate_follows_registration() {
        let open = build_node(CommandSpec::new("echo"), "p", "");
        assert!(open.accepts_prefix("/"));
        assert!(open.accepts_prefix(""));
        let strict = build_node(CommandSpec::new("echo").prefix("/"), "p", "");
        assert!(strict.accepts_prefix("/"));
        assert!(!strict.accepts_prefix("#"));
    }

    #[test]
    fn macro_meta_admin_becomes_role() {
        let spec =
            CommandSpec::from_macro(("/echo", true), sync_handler(|_ctx: &Invocation| Ok(None)));
        assert_eq!(spec.name, "/echo");
        assert_eq!(spec.role, Some(RoleRequired::Admin));
        assert!(spec.handler.is_some());
        let spec =
            CommandSpec::from_macro(("/echo", false), sync_handler(|_ctx: &Invocation| Ok(None)));
        assert_eq!(spec.role, None);
    }

    #[test]
    fn plugin_bridge_reads_sdk_meta() {
        struct Demo;
        impl Plugin for Demo {
            fn id(&self) -> &'static str {
                "demo"
            }
        }
        let commands = PluginCommands::from_plugin(&Demo, vec![CommandSpec::new("echo")]);
        assert_eq!(commands.id, "demo");
        assert_eq!(commands.name, "demo");
        assert_eq!(commands.description, "demo");
        assert_eq!(commands.commands.len(), 1);
    }
}
