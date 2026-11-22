use std::sync::{Arc, Mutex};

use aurorix_macros::command;
use aurorix_plugin::{
    help_spec, render_help, render_plugin_help, sync_handler, CommandRegistry, CommandSpec,
    Incoming, Invocation, Maintenance, Outgoing, PluginCommands, PluginError, ReplyKind, ReplySink,
    Router, Scene, Stage, Target,
};
use aurorix_template::Registry;

#[command("/pong")]
fn pong_handler(_ctx: &Invocation) -> Result<Option<Outgoing>, PluginError> {
    Ok(Some(Outgoing::new(ReplyKind::Handler, "pong")))
}

fn sample_registry() -> Arc<CommandRegistry> {
    let registry = Arc::new(CommandRegistry::new());
    registry.register_plugin(
        PluginCommands::new("arcaea", "查分")
            .describe("查分插件")
            .command(
                CommandSpec::new("a")
                    .alias("arc")
                    .describe("查分")
                    .subcommand(CommandSpec::new("b50").describe("最好 50 张"))
                    .subcommand(CommandSpec::new("song").alias("歌").describe("查歌")),
            )
            .command(CommandSpec::new("bind").describe("绑定账号")),
    );
    registry.register_plugin(
        PluginCommands::new("echo", "复读")
            .describe("复读插件")
            .command(CommandSpec::new("echo").alias("复读").describe("复读原文"))
            .command(CommandSpec::new("say")),
    );
    registry.register_plugin(
        PluginCommands::new("help", "帮助")
            .describe("帮助插件")
            .command(help_spec(Arc::clone(&registry))),
    );
    registry
}

#[test]
fn help_snapshot_groups_by_plugin_and_sorts() {
    let registry = sample_registry();
    let expected = concat!(
        "### 指令帮助\n\n",
        "#### arcaea · 查分插件\n",
        "- `/a`（别名：`arc`） — 查分\n",
        "  - `/a b50` — 最好 50 张\n",
        "  - `/a song`（别名：`歌`） — 查歌\n",
        "- `/bind` — 绑定账号\n\n",
        "#### echo · 复读插件\n",
        "- `/echo`（别名：`复读`） — 复读原文\n",
        "- `/say`\n\n",
        "#### help · 帮助插件\n",
        "- `/help`（别名：`帮助`） — 查看全部指令",
    );
    assert_eq!(render_help(&registry), expected);
}

#[test]
fn help_sections_are_sorted_by_plugin_id() {
    let registry = sample_registry();
    let text = render_help(&registry);
    let arcaea = text.find("#### arcaea").expect("要有 arcaea 段");
    let echo = text.find("#### echo").expect("要有 echo 段");
    let help = text.find("#### help").expect("要有 help 段");
    assert!(arcaea < echo && echo < help, "插件段要按 id 排序");
    assert_eq!(
        registry
            .plugins()
            .into_iter()
            .map(|plugin| plugin.id)
            .collect::<Vec<String>>(),
        vec!["arcaea".to_string(), "echo".to_string(), "help".to_string()]
    );
}

#[test]
fn help_lists_five_commands() {
    let registry = sample_registry();
    let text = render_plugin_help(&registry, "arcaea").expect("插件帮助要能单独渲染");
    assert!(text.contains("#### arcaea · 查分插件"));
    assert_eq!(
        registry.command_count(),
        5,
        "三个插件一共 5 条根指令: a/bind/echo/say/help"
    );
    assert!(render_plugin_help(&registry, "missing").is_none());
}

#[tokio::test]
async fn help_command_is_dispatchable() {
    let registry = sample_registry();
    let router = Router::new(Arc::clone(&registry));
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/help",
    );
    let outcome = router.dispatch_detailed(event).await;
    assert_eq!(outcome.stage, Stage::Handled);
    let reply = outcome.reply.expect("帮助要回话");
    assert!(reply.body().contains("#### arcaea"));
    assert_eq!(reply.kind, ReplyKind::Handler);
}

#[tokio::test]
async fn help_command_alias_works() {
    let registry = sample_registry();
    let router = Router::new(Arc::clone(&registry));
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/帮助",
    );
    assert_eq!(router.dispatch_detailed(event).await.stage, Stage::Handled);
}

#[test]
fn help_displays_configured_prefix() {
    let book = Arc::new(aurorix_core::PrefixBook::new());
    book.set(vec!["#".to_string()]);
    let registry = Arc::new(CommandRegistry::with_prefixes(Arc::clone(&book)));
    registry.register("demo", CommandSpec::new("ping").describe("探活"));
    let text = render_help(&registry);
    assert!(
        text.contains("- `#ping` — 探活"),
        "帮助里的前缀要跟着配置走"
    );
}

#[tokio::test]
async fn argument_error_uses_card_and_usage() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("echo")
            .with_args(
                aurorix_plugin::ArgsSpec::new()
                    .positional(aurorix_plugin::PositionalSpec::required("name")),
            )
            .on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry);
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/echo",
    );
    let outcome = router.dispatch_detailed(event).await;
    assert_eq!(outcome.stage, Stage::ArgumentError);
    let reply = outcome.reply.expect("参数错误要回话");
    assert_eq!(reply.kind, ReplyKind::ArgumentError);
    assert!(reply.body().contains("missing positional arguments <name>"));
    assert!(reply.body().contains("用法: echo"));
    assert!(reply.at);
    assert!(!reply.no_footer, "错误卡片带底部按钮");
}

#[tokio::test]
async fn argument_error_falls_back_when_template_missing() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("echo")
            .with_args(
                aurorix_plugin::ArgsSpec::new()
                    .positional(aurorix_plugin::PositionalSpec::required("name")),
            )
            .on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry);
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/echo",
    );
    let reply = router.dispatch(event).await.expect("模板缺失也要有出口");
    assert!(reply.body().contains("❌ 指令参数错误"));
}

#[tokio::test]
async fn argument_error_uses_registered_error_card() {
    let templates = Arc::new(Registry::new());
    templates.register(
        aurorix_template::TemplateKind::Markdown,
        "demo",
        "error_card",
        "自定义 {{title}} / {{#each fields}}{{label}}={{content}};{{/each}}",
    );
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("echo")
            .with_args(
                aurorix_plugin::ArgsSpec::new()
                    .positional(aurorix_plugin::PositionalSpec::required("name")),
            )
            .on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry).with_templates(templates);
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/echo",
    );
    let reply = router.dispatch(event).await.expect("要有回复");
    assert!(
        reply.body().starts_with("自定义 ❌ 指令参数错误"),
        "插件模板优先"
    );
    assert!(reply
        .body()
        .contains("原因=missing positional arguments <name>"));
}

#[tokio::test]
async fn handler_error_without_templates_still_answers() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("boom").on_sync(|_ctx: &Invocation| Err(PluginError::handler("炸了"))),
    );
    let router = Router::new(registry);
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/boom",
    );
    let reply = router.dispatch(event).await.expect("错误也要有出口");
    assert!(!reply.body().trim().is_empty());
    assert!(reply.body().contains("炸了"));
}

#[tokio::test]
async fn handler_can_stay_silent() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("quiet").on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry);
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/quiet",
    );
    let outcome = router.dispatch_detailed(event).await;
    assert_eq!(outcome.stage, Stage::Handled);
    assert!(outcome.reply.is_none(), "处理器自己决定不回话");
}

#[derive(Debug, Default)]
struct RecordingSink {
    seen: Mutex<Vec<(String, Outgoing)>>,
}

impl ReplySink for RecordingSink {
    async fn reply(&self, ctx: &Invocation, message: Outgoing) -> Result<(), PluginError> {
        let mut guard = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        guard.push((ctx.command.clone(), message));
        Ok(())
    }
}

#[tokio::test]
async fn dispatch_with_sends_through_the_sink() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping")
            .on_sync(|_ctx: &Invocation| Ok(Some(Outgoing::new(ReplyKind::Handler, "pong")))),
    );
    let router = Router::new(registry);
    let sink = RecordingSink::default();
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/ping",
    );
    let reply = router
        .dispatch_with(&sink, event)
        .await
        .expect("发送要成功");
    assert!(reply.is_some());
    let seen = sink.seen.lock().unwrap_or_else(|p| p.into_inner());
    assert_eq!(seen.len(), 1, "只发一次");
    assert_eq!(seen[0].0, "ping");
    assert_eq!(seen[0].1.body(), "pong");
}

#[tokio::test]
async fn dispatch_with_skips_silent_outcomes() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping").on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry);
    let sink = RecordingSink::default();
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/ping",
    );
    let reply = router.dispatch_with(&sink, event).await.expect("不该报错");
    assert!(reply.is_none());
    assert!(
        sink.seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty(),
        "静默时不该发消息"
    );
}

#[tokio::test]
async fn dispatch_with_reports_sink_failure() {
    struct FailingSink;

    impl ReplySink for FailingSink {
        async fn reply(&self, _ctx: &Invocation, _message: Outgoing) -> Result<(), PluginError> {
            Err(PluginError::reply("通道断了"))
        }
    }

    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping")
            .on_sync(|_ctx: &Invocation| Ok(Some(Outgoing::new(ReplyKind::Handler, "pong")))),
    );
    let router = Router::new(registry);
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/ping",
    );
    let error = router
        .dispatch_with(&FailingSink, event)
        .await
        .expect_err("发送失败要往上抛");
    assert_eq!(error.to_string(), "回复发送失败: 通道断了");
}

#[tokio::test]
async fn macro_registered_command_is_dispatchable() {
    let registry = Arc::new(CommandRegistry::new());
    let handler: Arc<dyn aurorix_plugin::CommandHandler> = sync_handler(pong_handler);
    registry.register(
        "macro",
        CommandSpec::from_macro(__AURORIX_CMD_PONG_HANDLER, handler),
    );
    let router = Router::new(Arc::clone(&registry));
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/pong",
    );
    let outcome = router.dispatch_detailed(event).await;
    assert_eq!(outcome.stage, Stage::Handled);
    assert_eq!(outcome.invocation.command, "pong");
    assert_eq!(outcome.text().as_deref(), Some("pong"));
}

#[tokio::test]
async fn global_maintenance_text_is_normalized() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping").on_sync(|_: &Invocation| Ok(None)),
    );
    let router = Router::new(registry);
    router.set_global_maintenance(Maintenance {
        enabled: true,
        message: "  ".to_string(),
    });
    let event = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        "/ping",
    );
    assert_eq!(
        router.dispatch(event).await.map(|reply| reply.body()),
        Some(aurorix_core::DEFAULT_MAINTENANCE_MESSAGE.to_string())
    );
    router.set_global_maintenance(Maintenance::default());
    assert_eq!(router.global_maintenance().text(), None);
}
