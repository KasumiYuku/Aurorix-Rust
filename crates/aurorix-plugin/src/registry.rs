use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use aurorix_core::PrefixBook;

use crate::command::{
    build_node, normalize_name, prefix_symbol_of, CommandNode, CommandSpec, PluginCommands,
};
use crate::config_field::PluginConfigField;

#[derive(Debug, Clone)]
pub struct PluginSlot {
    pub id: String,
    pub name: String,
    pub description: String,
    pub commands: Vec<Arc<CommandNode>>,
    pub config: Vec<PluginConfigField>,
}

#[derive(Debug, Clone, Default)]
pub struct RegistryIndex {
    commands: BTreeMap<String, Arc<CommandNode>>,
    aliases: BTreeMap<String, Arc<CommandNode>>,
    names: Vec<String>,
    plugins: BTreeMap<String, PluginSlot>,
}

impl RegistryIndex {
    pub fn lookup(&self, name: &str) -> Option<Arc<CommandNode>> {
        match self.commands.get(name) {
            Some(node) => Some(Arc::clone(node)),
            None => self.aliases.get(name).map(Arc::clone),
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.names.clone()
    }

    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    pub fn alias_count(&self) -> usize {
        self.aliases.len()
    }

    pub fn plugin(&self, id: &str) -> Option<PluginSlot> {
        self.plugins.get(id).cloned()
    }

    pub fn plugins(&self) -> Vec<PluginSlot> {
        self.plugins.values().cloned().collect()
    }

    pub fn command_paths(&self, plugin: &str) -> BTreeSet<String> {
        let mut paths = BTreeSet::new();
        if let Some(slot) = self.plugins.get(plugin) {
            for command in &slot.commands {
                collect_paths(command, &mut paths);
            }
        }
        paths
    }

    fn publish_names(&mut self) {
        let mut names: Vec<String> = self.commands.keys().cloned().collect();
        names.extend(self.aliases.keys().cloned());
        names.sort();
        names.dedup();
        self.names = names;
    }

    fn match_command_name(
        &self,
        token: &str,
        allow_bare: bool,
        prefixes: &[String],
    ) -> Option<(Arc<CommandNode>, String)> {
        if !allow_bare && prefix_symbol_of(token, prefixes).is_none() {
            return None;
        }
        let used = prefix_symbol_of(token, prefixes).unwrap_or_default();
        if let Some(node) = self.commands.get(token) {
            return Some((Arc::clone(node), used));
        }
        if let Some(node) = self.aliases.get(token) {
            return Some((Arc::clone(node), used));
        }
        for prefix in prefixes {
            if prefix.is_empty()
                || !token.starts_with(prefix.as_str())
                || token.len() <= prefix.len()
            {
                continue;
            }
            let rest = &token[prefix.len()..];
            if let Some(node) = self.commands.get(rest) {
                return Some((Arc::clone(node), prefix.clone()));
            }
            if let Some(node) = self.aliases.get(rest) {
                return Some((Arc::clone(node), prefix.clone()));
            }
        }
        None
    }

    fn glued(
        &self,
        token: &str,
        prefixes: &[String],
    ) -> Option<(Arc<CommandNode>, String, Vec<String>, String)> {
        let prefix = prefix_symbol_of(token, prefixes)?;
        let name = &token[prefix.len()..];
        if name.is_empty() {
            return None;
        }
        let boundary = self
            .names
            .partition_point(|candidate| candidate.as_str() < name);
        let mut hit: Option<(Arc<CommandNode>, String, String)> = None;
        let mut longest: Option<(Arc<CommandNode>, String, String)> = None;
        for position in (0..boundary).rev() {
            let candidate = &self.names[position];
            if name.len() <= candidate.len() {
                continue;
            }
            if !name.starts_with(candidate.as_str()) {
                continue;
            }
            let Some(node) = self.lookup(candidate) else {
                continue;
            };
            let rest = name[candidate.len()..].to_string();
            if hit.is_none()
                && (node.children.keys().any(|key| rest.starts_with(key.as_str()))
                    || glued_chain(&node, rest.as_str()).is_some())
            {
                hit = Some((Arc::clone(&node), prefix.clone(), rest.clone()));
            }
            if longest.is_none() {
                longest = Some((node, prefix.clone(), rest));
            }
        }
        let (node, used, rest) = hit.or(longest)?;
        let (_, names, leftover) = split_chain(&node, rest.as_str());
        let glued = names.concat();
        let mut tokens = names;
        if !leftover.is_empty() {
            tokens.push(leftover);
        }
        Some((node, used, tokens, glued))
    }
}

fn collect_paths(node: &Arc<CommandNode>, paths: &mut BTreeSet<String>) {
    paths.insert(node.path.clone());
    for child in &node.subcommands {
        collect_paths(child, paths);
    }
}

#[derive(Debug, Clone)]
pub struct RootMatch {
    pub node: Arc<CommandNode>,
    pub rest: Vec<String>,
    pub prefix: String,
    pub glued: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubMatch {
    pub path: String,
    pub consumed: usize,
}

pub struct CommandRegistry {
    index: ArcSwap<RegistryIndex>,
    prefixes: Arc<PrefixBook>,
    write_lock: Mutex<()>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRegistry")
            .field("commands", &self.index.load().command_count())
            .finish()
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::with_prefixes(Arc::new(PrefixBook::new()))
    }

    pub fn with_prefixes(prefixes: Arc<PrefixBook>) -> Self {
        Self {
            index: ArcSwap::from_pointee(RegistryIndex::default()),
            prefixes,
            write_lock: Mutex::new(()),
        }
    }

    pub fn prefix_book(&self) -> Arc<PrefixBook> {
        Arc::clone(&self.prefixes)
    }

    pub fn prefix_chars(&self) -> Vec<String> {
        self.prefixes.get().as_ref().clone()
    }

    pub fn has_bare(&self) -> bool {
        self.prefixes.has_bare()
    }

    pub fn display_prefix(&self) -> String {
        self.prefix_chars()
            .into_iter()
            .find(|prefix| !prefix.is_empty())
            .unwrap_or_default()
    }

    pub fn index(&self) -> Arc<RegistryIndex> {
        self.index.load_full()
    }

    pub fn names(&self) -> Vec<String> {
        self.index().names()
    }

    pub fn command_count(&self) -> usize {
        self.index().command_count()
    }

    pub fn plugin(&self, id: &str) -> Option<PluginSlot> {
        self.index().plugin(id)
    }

    pub fn plugins(&self) -> Vec<PluginSlot> {
        self.index().plugins()
    }

    pub fn command_paths(&self, plugin: &str) -> BTreeSet<String> {
        self.index().command_paths(plugin)
    }

    pub fn register_plugin(&self, plugin: PluginCommands) -> Vec<String> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = RegistryIndex::clone(self.index.load_full().as_ref());
        let mut warnings = Vec::new();
        let mut nodes = Vec::new();
        for spec in plugin.commands {
            let node = build_node(spec, &plugin.id, "");
            if let Some(existing) = next.commands.get(&node.name) {
                if existing.plugin != plugin.id {
                    warnings.push(format!(
                        "指令 {} 与插件 {} 冲突, 由 {} 覆盖",
                        node.name, existing.plugin, plugin.id
                    ));
                }
            }
            next.commands.insert(node.name.clone(), Arc::clone(&node));
            for alias in &node.aliases {
                let key = normalize_name(alias);
                if let Some(existing) = next.aliases.get(&key) {
                    if existing.plugin != plugin.id {
                        warnings.push(format!(
                            "别名 {alias} 与插件 {} 冲突, 由 {} 覆盖",
                            existing.plugin, plugin.id
                        ));
                    }
                }
                next.aliases.insert(key, Arc::clone(&node));
            }
            nodes.push(node);
        }
        next.plugins.insert(
            plugin.id.clone(),
            PluginSlot {
                id: plugin.id,
                name: plugin.name,
                description: plugin.description,
                commands: nodes,
                config: plugin.config,
            },
        );
        next.publish_names();
        self.index.store(Arc::new(next));
        warnings
    }

    pub fn register(&self, plugin_id: &str, spec: CommandSpec) -> Vec<String> {
        self.register_plugin(PluginCommands::new(plugin_id, plugin_id).command(spec))
    }

    pub fn get(&self, name: &str) -> Option<Arc<CommandNode>> {
        self.index().lookup(name)
    }

    pub fn match_command(&self, token: &str) -> Option<Arc<CommandNode>> {
        let prefixes = self.prefix_chars();
        let index = self.index();
        index
            .match_command_name(token, self.prefixes.has_bare(), &prefixes)
            .map(|(node, _)| node)
    }

    pub fn resolve_root(&self, tokens: &[String]) -> Option<RootMatch> {
        let first = tokens.first()?;
        let prefixes = self.prefix_chars();
        let index = self.index();
        let allow_bare = prefixes.iter().any(|prefix| prefix.is_empty());
        if prefixes
            .iter()
            .any(|prefix| !prefix.is_empty() && prefix == first)
        {
            let second = tokens.get(1)?;
            let (node, prefix) = index.match_command_name(second, true, &prefixes)?;
            if !node.accepts_prefix(&prefix) {
                return None;
            }
            return Some(RootMatch {
                node,
                rest: tokens[2..].to_vec(),
                prefix,
                glued: String::new(),
            });
        }
        if let Some((node, prefix)) = index.match_command_name(first, allow_bare, &prefixes) {
            if !node.accepts_prefix(&prefix) {
                return None;
            }
            return Some(RootMatch {
                node,
                rest: tokens[1..].to_vec(),
                prefix,
                glued: String::new(),
            });
        }
        let (node, prefix, mut rest, glued) = index.glued(first, &prefixes)?;
        if !node.accepts_prefix(&prefix) {
            return None;
        }
        rest.extend(tokens[1..].iter().cloned());
        Some(RootMatch {
            node,
            rest,
            prefix,
            glued,
        })
    }

    pub fn resolve(
        &self,
        root: Arc<CommandNode>,
        tokens: &[String],
    ) -> (Arc<CommandNode>, String, usize) {
        let prefixes = self.prefix_chars();
        let mut current = root;
        let mut path = current.path.clone();
        let mut consumed = 0usize;
        while consumed < tokens.len() && !current.children.is_empty() {
            let token = bare_token(&tokens[consumed], &prefixes);
            let mut next = current.children.get(token).cloned();
            if next.is_none() {
                for prefix in &prefixes {
                    if prefix.is_empty()
                        || !token.starts_with(prefix.as_str())
                        || token.len() <= prefix.len()
                    {
                        continue;
                    }
                    if let Some(node) = current.children.get(&token[prefix.len()..]) {
                        next = Some(Arc::clone(node));
                        break;
                    }
                }
            }
            if next.is_none() {
                if let Some((node, glued_path)) = glued_chain(&current, token) {
                    return (
                        node,
                        format!("{path} {glued_path}"),
                        consumed + 1,
                    );
                }
            }
            let Some(node) = next else {
                break;
            };
            path = format!("{path} {}", node.name);
            current = node;
            consumed += 1;
        }
        (current, path, consumed)
    }
}

fn bare_token<'a>(token: &'a str, prefixes: &[String]) -> &'a str {
    for prefix in prefixes {
        if prefix.is_empty() || !token.starts_with(prefix.as_str()) || token.len() <= prefix.len() {
            continue;
        }
        return &token[prefix.len()..];
    }
    token
}

fn split_chain(node: &Arc<CommandNode>, token: &str) -> (Arc<CommandNode>, Vec<String>, String) {
    let mut current = Arc::clone(node);
    let mut rest = token;
    let mut names: Vec<String> = Vec::new();
    while !rest.is_empty() && !current.children.is_empty() {
        let mut best: Option<(usize, &Arc<CommandNode>)> = None;
        for (name, child) in &current.children {
            if name.len() > rest.len() || !rest.starts_with(name.as_str()) {
                continue;
            }
            if best.is_none_or(|(width, _)| name.len() > width) {
                best = Some((name.len(), child));
            }
        }
        let Some((width, child)) = best else {
            break;
        };
        names.push(rest[..width].to_string());
        rest = &rest[width..];
        current = Arc::clone(child);
    }
    (current, names, rest.to_string())
}

fn glued_chain(node: &Arc<CommandNode>, token: &str) -> Option<(Arc<CommandNode>, String)> {
    let (current, names, leftover) = split_chain(node, token);
    if !leftover.is_empty() || names.is_empty() {
        return None;
    }
    Some((current, names.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> CommandRegistry {
        let registry = CommandRegistry::new();
        registry.register_plugin(
            PluginCommands::new("demo", "示例")
                .describe("示例插件")
                .command(CommandSpec::new("/echo").alias("复读").describe("复读机"))
                .command(
                    CommandSpec::new("p")
                        .alias("play")
                        .subcommand(CommandSpec::new("song").alias("歌"))
                        .subcommand(CommandSpec::new("b50")),
                ),
        );
        registry
    }

    fn tokens(input: &str) -> Vec<String> {
        input.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn resolves_slash_hash_and_bare() {
        let registry = registry();
        for input in ["/echo hi", "#echo hi", "echo hi"] {
            let matched = registry
                .resolve_root(&tokens(input))
                .unwrap_or_else(|| panic!("{input} 应当命中"));
            assert_eq!(matched.node.path, "echo");
            assert_eq!(matched.rest, vec!["hi".to_string()]);
        }
    }

    #[test]
    fn resolves_alias() {
        let registry = registry();
        for input in ["/复读 hi", "#复读 hi", "复读 hi"] {
            let matched = registry.resolve_root(&tokens(input)).expect("别名应当命中");
            assert_eq!(matched.node.name, "echo");
        }
    }

    #[test]
    fn resolves_subcommand_and_path() {
        let registry = registry();
        let root = registry
            .resolve_root(&tokens("/p song Lyrith"))
            .expect("根指令要命中");
        let (node, path, consumed) = registry.resolve(Arc::clone(&root.node), &root.rest);
        assert_eq!(path, "p song");
        assert_eq!(node.name, "song");
        assert_eq!(consumed, 1);
        assert_eq!(root.rest[consumed], "Lyrith");
    }

    #[test]
    fn subcommand_alias_and_symbol_sinking() {
        let registry = registry();
        let root = registry
            .resolve_root(&tokens("/p /歌 x"))
            .expect("根指令要命中");
        let (node, path, consumed) = registry.resolve(Arc::clone(&root.node), &root.rest);
        assert_eq!(node.name, "song");
        assert_eq!(path, "p song");
        assert_eq!(consumed, 1);
    }

    #[test]
    fn unknown_command_is_none() {
        let registry = registry();
        assert!(registry.resolve_root(&tokens("/nope")).is_none());
        assert!(registry.resolve_root(&tokens("")).is_none());
        assert!(registry.resolve_root(&tokens("/")).is_none());
    }

    #[test]
    fn matching_is_case_sensitive() {
        let registry = registry();
        assert!(registry.resolve_root(&tokens("/echo")).is_some());
        assert!(registry.resolve_root(&tokens("/ECHO")).is_none());
        assert!(registry.resolve_root(&tokens("/Echo")).is_none());
    }

    #[test]
    fn prefix_gate_blocks_other_symbols() {
        let registry = CommandRegistry::new();
        registry.register_plugin(
            PluginCommands::new("demo", "示例")
                .command(CommandSpec::new("only").prefix("/"))
                .command(CommandSpec::new("other")),
        );
        assert!(registry.resolve_root(&tokens("/only")).is_some());
        assert!(registry.resolve_root(&tokens("#only")).is_none());
        assert!(registry.resolve_root(&tokens("only")).is_none());
        assert!(registry.resolve_root(&tokens("#other")).is_some());
    }

    #[test]
    fn glued_longest_candidate_wins() {
        let registry = CommandRegistry::new();
        registry.register_plugin(
            PluginCommands::new("demo", "示例")
                .command(CommandSpec::new("a"))
                .command(CommandSpec::new("as"))
                .command(CommandSpec::new("admin")),
        );
        let matched = registry
            .resolve_root(&tokens("/asong x"))
            .expect("最长候选应当命中");
        assert_eq!(matched.node.name, "as");
        assert_eq!(matched.rest, vec!["ong".to_string(), "x".to_string()]);
        let matched = registry
            .resolve_root(&tokens("/asong"))
            .expect("粘合词应当命中");
        assert_eq!(matched.node.name, "as");
        assert_eq!(matched.rest, vec!["ong".to_string()]);
        let matched = registry
            .resolve_root(&tokens("/admin"))
            .expect("精确命中优先");
        assert_eq!(matched.node.name, "admin");
    }

    #[test]
    fn alias_participates_in_glue() {
        let registry = CommandRegistry::new();
        registry.register_plugin(
            PluginCommands::new("demo", "示例")
                .command(CommandSpec::new("p").alias("phibind"))
                .command(CommandSpec::new("ph")),
        );
        let matched = registry
            .resolve_root(&tokens("/phibindx"))
            .expect("别名也要参与粘合");
        assert_eq!(matched.node.name, "p");
        assert_eq!(matched.rest, vec!["x".to_string()]);
    }

    #[test]
    fn glued_subcommand_splits_inside_token() {
        let registry = CommandRegistry::new();
        registry.register_plugin(
            PluginCommands::new("demo", "示例").command(
                CommandSpec::new("a").subcommands(vec![CommandSpec::new("b50").subcommands(
                    vec![CommandSpec::new("last"), CommandSpec::new("-f")],
                )]),
            ),
        );
        let glued = registry.resolve_root(&tokens("/ab50last")).expect("根要粘合命中");
        assert_eq!(glued.node.name, "a");
        assert_eq!(
            glued.rest,
            vec!["b50".to_string(), "last".to_string()]
        );
        assert_eq!(glued.glued, "b50last");
        let (node, path, consumed) = registry.resolve(Arc::clone(&glued.node), &glued.rest);
        assert_eq!(path, "a b50 last");
        assert_eq!(node.name, "last");
        assert_eq!(consumed, 2);
        let spaced = registry.resolve_root(&tokens("/a b50 last")).expect("空格写法");
        let (node, path, consumed) = registry.resolve(Arc::clone(&spaced.node), &spaced.rest);
        assert_eq!(path, "a b50 last");
        assert_eq!(node.name, "last");
        assert_eq!(consumed, 2);
        let force = registry.resolve_root(&tokens("/ab50-f")).expect("根仍要命中");
        let (node, _, _) = registry.resolve(Arc::clone(&force.node), &force.rest);
        assert_eq!(node.name, "-f");
    }

    #[test]
    fn glued_root_prefers_child_hit_over_longer_alias() {
        let registry = CommandRegistry::new();
        registry.register_plugin(
            PluginCommands::new("demo", "示例")
                .command(
                    CommandSpec::new("a")
                        .alias("arcaea")
                        .subcommand(CommandSpec::new("chart")),
                )
                .command(
                    CommandSpec::new("ca")
                        .alias("ac")
                        .subcommand(CommandSpec::new("chart")),
                ),
        );
        let glued = registry
            .resolve_root(&tokens("/achart光影byd"))
            .expect("粘合根要命中");
        assert_eq!(glued.node.name, "a");
        assert_eq!(
            glued.rest,
            vec!["chart".to_string(), "光影byd".to_string()]
        );
        assert_eq!(glued.glued, "chart");
        let (node, path, consumed) = registry.resolve(Arc::clone(&glued.node), &glued.rest);
        assert_eq!(path, "a chart");
        assert_eq!(node.name, "chart");
        assert_eq!(consumed, 1);
        let glued = registry
            .resolve_root(&tokens("/acchart光影byd"))
            .expect("国服别名要命中最长根");
        assert_eq!(glued.node.name, "ca");
        let (node, path, _) = registry.resolve(Arc::clone(&glued.node), &glued.rest);
        assert_eq!(path, "ca chart");
        assert_eq!(node.name, "chart");
    }

    #[test]
    fn conflicting_registration_warns_and_overrides() {
        let registry = CommandRegistry::new();
        assert!(registry
            .register("first", CommandSpec::new("echo"))
            .is_empty());
        let warnings = registry.register("second", CommandSpec::new("echo"));
        assert_eq!(warnings.len(), 1, "同名指令要报冲突");
        assert!(warnings[0].contains("first"), "冲突信息要带旧插件");
        assert_eq!(
            registry.get("echo").map(|node| node.plugin.clone()),
            Some("second".to_string())
        );
    }

    #[test]
    fn index_lists_plugins_and_paths() {
        let registry = registry();
        let plugins = registry.plugins();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "示例");
        assert_eq!(plugins[0].description, "示例插件");
        assert_eq!(plugins[0].commands.len(), 2);
        let paths = registry.command_paths("demo");
        assert!(paths.contains("p"));
        assert!(paths.contains("p song"));
        assert!(paths.contains("p b50"));
        assert_eq!(registry.command_count(), 2);
        assert!(registry.command_paths("missing").is_empty());
    }

    #[test]
    fn names_are_sorted_and_deduplicated() {
        let registry = registry();
        let names = registry.names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "名字表要排序");
        assert!(names.contains(&"echo".to_string()));
        assert!(names.contains(&"play".to_string()));
        assert!(names.contains(&"复读".to_string()));
    }

    #[test]
    fn display_prefix_is_first_symbol() {
        let registry = registry();
        assert_eq!(registry.display_prefix(), "/");
        assert!(registry.has_bare());
        let strict = Arc::new(PrefixBook::new());
        strict.set(vec!["#".to_string()]);
        let registry = CommandRegistry::with_prefixes(strict);
        assert_eq!(registry.display_prefix(), "#");
        assert!(!registry.has_bare());
        assert_eq!(registry.prefix_chars(), vec!["#".to_string()]);
    }

    #[test]
    fn bare_match_requires_bare_prefix() {
        let book = Arc::new(PrefixBook::new());
        book.set(vec!["/".to_string()]);
        let registry = CommandRegistry::with_prefixes(book);
        registry.register("demo", CommandSpec::new("echo"));
        assert!(registry.resolve_root(&tokens("/echo")).is_some());
        assert!(registry.resolve_root(&tokens("echo")).is_none());
    }

    #[test]
    fn exact_symbol_prefix_needs_a_name() {
        let registry = CommandRegistry::new();
        registry.register("demo", CommandSpec::new("echo"));
        assert!(registry.resolve_root(&tokens("/ echo hi")).is_some());
        let matched = registry
            .resolve_root(&tokens("/ echo hi"))
            .expect("符号独立成词要命中");
        assert_eq!(matched.rest, vec!["hi".to_string()]);
        assert!(registry.resolve_root(&tokens("# ")).is_none());
    }
}
