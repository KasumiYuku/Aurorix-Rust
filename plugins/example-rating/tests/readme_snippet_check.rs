use aurorix_plugin::{
    CommandSpec, Invocation, Outgoing, PluginCommands, PluginError, ReplyKind, RoleRequired,
};

pub fn plugin() -> PluginCommands {
    PluginCommands::new("hello", "问候")
        .describe("示例插件: 累计问候次数")
        .command(
            CommandSpec::new("/hello")
                .role(RoleRequired::Member)
                .describe("打个招呼, 累计调用次数")
                .on(|inv: Invocation| async move {
                    let kv = inv
                        .kv()
                        .ok_or_else(|| PluginError::handler("插件库未就绪"))?;
                    let hits = kv.incr("hits", 1).await.map_err(fail)?;
                    let saved = kv.get::<String>("greeting").await.map_err(fail)?;
                    let greeting = saved
                        .filter(|text| !text.is_empty())
                        .unwrap_or_else(|| "你好".to_string());
                    let who = inv.username.as_deref().unwrap_or(&inv.user);
                    let text = format!("{greeting}, {who} · 第 {hits} 次");
                    Ok(Some(Outgoing::new(ReplyKind::Handler, text)))
                }),
        )
}

fn fail(err: impl std::fmt::Display) -> PluginError {
    PluginError::handler(err.to_string())
}

#[test]
fn readme_snippet_is_well_formed() {
    let commands = plugin();
    assert_eq!(commands.id, "hello");
    assert_eq!(commands.commands.len(), 1);
}
