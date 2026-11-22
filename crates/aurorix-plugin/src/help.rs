use std::sync::Arc;

use crate::command::{CommandNode, CommandSpec};
use crate::registry::{CommandRegistry, PluginSlot};
use crate::reply::{Invocation, Outgoing, ReplyKind};

pub const HELP_HEADER: &str = "### 指令帮助";
pub const HELP_NAME: &str = "help";
pub const HELP_ALIAS: &str = "帮助";
pub const HELP_DESCRIBE: &str = "查看全部指令";

pub fn render_help(registry: &CommandRegistry) -> String {
    let prefix = registry.display_prefix();
    let mut blocks = vec![HELP_HEADER.to_string()];
    for plugin in registry.plugins() {
        blocks.push(render_plugin_block(&plugin, &prefix));
    }
    blocks.join("\n\n")
}

pub fn render_plugin_help(registry: &CommandRegistry, plugin: &str) -> Option<String> {
    let slot = registry.plugin(plugin)?;
    Some(render_plugin_block(&slot, &registry.display_prefix()))
}

fn render_plugin_block(slot: &PluginSlot, prefix: &str) -> String {
    let mut lines = vec![plugin_title(slot)];
    let mut commands = slot.commands.clone();
    commands.sort_by(|left, right| left.path.cmp(&right.path));
    for command in &commands {
        push_command_lines(command, prefix, 0, &mut lines);
    }
    lines.join("\n")
}

fn plugin_title(slot: &PluginSlot) -> String {
    if slot.description.is_empty() {
        format!("#### {}", slot.id)
    } else {
        format!("#### {} · {}", slot.id, slot.description)
    }
}

fn push_command_lines(
    node: &Arc<CommandNode>,
    prefix: &str,
    depth: usize,
    lines: &mut Vec<String>,
) {
    let indent = "  ".repeat(depth);
    lines.push(format!("{indent}- {}", command_line(node, prefix)));
    let mut children = node.subcommands.clone();
    children.sort_by(|left, right| left.path.cmp(&right.path));
    for child in &children {
        push_command_lines(child, prefix, depth + 1, lines);
    }
}

fn command_line(node: &CommandNode, prefix: &str) -> String {
    let mut line = format!("`{prefix}{}`", node.path);
    if !node.aliases.is_empty() {
        let aliases: Vec<String> = node
            .aliases
            .iter()
            .map(|alias| format!("`{alias}`"))
            .collect();
        line.push_str(&format!("（别名：{}）", aliases.join("、")));
    }
    if !node.help.is_empty() {
        line.push_str(" — ");
        line.push_str(&node.help);
    }
    line
}

pub fn help_spec(registry: Arc<CommandRegistry>) -> CommandSpec {
    CommandSpec::new(HELP_NAME)
        .alias(HELP_ALIAS)
        .describe(HELP_DESCRIBE)
        .on_sync(move |_ctx: &Invocation| {
            Ok(Some(Outgoing::new(
                ReplyKind::Handler,
                render_help(&registry),
            )))
        })
}
