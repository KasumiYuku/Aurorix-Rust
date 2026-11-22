use std::sync::Arc;
use std::time::Duration;

use aurorix_plugin::{
    AccessBook, AccessConfig, AccessMode, AccessRule, BanEntry, BanList, CommandRegistry,
    CommandSpec, Deny, Incoming, Invocation, Maintenance, Outgoing, PluginCommands, PluginError,
    Rate, RateLimiter, ReplyKind, RoleRequired, RoleResolver, Router, Scene, Stage,
    StaticRoleResolver, Target,
};

fn event(content: &str) -> Incoming {
    Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user("u1"),
        "u1",
        content,
    )
}

fn echo_router() -> Router {
    let registry = Arc::new(CommandRegistry::new());
    registry.register_plugin(
        PluginCommands::new("demo", "示例").command(
            CommandSpec::new("/echo")
                .alias("复读")
                .describe("复读原文")
                .on_sync(|ctx: &Invocation| {
                    Ok(Some(Outgoing::new(ReplyKind::Handler, ctx.text.clone())))
                })
                .subcommand(CommandSpec::new("sub").describe("子指令").on_sync(
                    |_ctx: &Invocation| Ok(Some(Outgoing::new(ReplyKind::Handler, "sub"))),
                )),
        ),
    );
    Router::new(registry)
}

#[tokio::test]
async fn routes_slash_hash_and_bare() {
    let router = echo_router();
    for content in ["/echo hi", "#echo hi", "echo hi"] {
        let outcome = router.dispatch_detailed(event(content)).await;
        assert_eq!(outcome.stage, Stage::Handled, "{content} 应当命中");
        assert_eq!(outcome.text().as_deref(), Some("hi"), "{content} 的参数要原样传下去");
    }
}

#[tokio::test]
async fn routes_alias_and_subcommand() {
    let router = echo_router();
    let outcome = router.dispatch_detailed(event("/复读 你好")).await;
    assert_eq!(outcome.stage, Stage::Handled);
    assert_eq!(outcome.text().as_deref(), Some("你好"));
    assert_eq!(outcome.invocation.command, "echo");
    let outcome = router.dispatch_detailed(event("/echo sub a b")).await;
    assert_eq!(outcome.stage, Stage::Handled);
    assert_eq!(outcome.text().as_deref(), Some("sub"));
    assert_eq!(outcome.invocation.command, "echo sub");
    assert_eq!(
        outcome.invocation.args.positional,
        vec!["a".to_string(), "b".to_string()]
    );
}

#[tokio::test]
async fn unknown_and_wrong_case_are_silent() {
    let router = echo_router();
    for content in ["/nope", "/ECHO hi", "nope"] {
        let outcome = router.dispatch_detailed(event(content)).await;
        assert_eq!(outcome.stage, Stage::ResolveRoot, "{content} 应当解析失败");
        assert!(outcome.silent(), "{content} 不该回话");
    }
    let empty = router.dispatch_detailed(event("   ")).await;
    assert!(empty.silent(), "空内容要静默");
}

#[tokio::test]
async fn same_name_works_on_every_prefix() {
    let router = echo_router();
    let slash = router.dispatch_detailed(event("/echo x")).await;
    let hash = router.dispatch_detailed(event("#echo x")).await;
    let bare = router.dispatch_detailed(event("echo x")).await;
    for outcome in [slash, hash, bare] {
        assert_eq!(outcome.stage, Stage::Handled);
        assert_eq!(outcome.invocation.command, "echo");
    }
}

#[tokio::test]
async fn prefix_gate_limits_symbols() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("only")
            .prefix("/")
            .on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry);
    assert_eq!(
        router.dispatch_detailed(event("/only")).await.stage,
        Stage::Handled
    );
    assert_eq!(
        router.dispatch_detailed(event("#only")).await.stage,
        Stage::ResolveRoot,
        "限定前缀的指令不该被别的符号命中"
    );
    assert_eq!(
        router.dispatch_detailed(event("only")).await.stage,
        Stage::ResolveRoot,
        "限定前缀的指令不该被裸名命中"
    );
}

fn matrix_router() -> Router {
    let registry = Arc::new(CommandRegistry::new());
    registry.register_plugin(
        PluginCommands::new("demo", "示例")
            .command(
                CommandSpec::new("needowner")
                    .role(RoleRequired::Owner)
                    .on_sync(|_ctx: &Invocation| {
                        Ok(Some(Outgoing::new(ReplyKind::Handler, "owner")))
                    }),
            )
            .command(
                CommandSpec::new("needadmin")
                    .role(RoleRequired::Admin)
                    .on_sync(|_ctx: &Invocation| {
                        Ok(Some(Outgoing::new(ReplyKind::Handler, "admin")))
                    }),
            )
            .command(
                CommandSpec::new("needmember")
                    .role(RoleRequired::Member)
                    .on_sync(|_ctx: &Invocation| {
                        Ok(Some(Outgoing::new(ReplyKind::Handler, "member")))
                    }),
            ),
    );
    let roles: Arc<dyn RoleResolver> = Arc::new(
        StaticRoleResolver::new()
            .with_user("owner", RoleRequired::Owner)
            .with_user("admin", RoleRequired::Admin)
            .with_user("member", RoleRequired::Member),
    );
    Router::new(registry).with_roles(roles)
}

async fn resolve_for(router: &Router, user: &str, command: &str) -> Stage {
    let incoming = Incoming::new(
        Scene::Group,
        Target::new(Scene::Group, "g1").with_user(user),
        user,
        format!("/{command}"),
    );
    router.dispatch_detailed(incoming).await.stage
}

#[tokio::test]
async fn permission_matrix_covers_four_identities() {
    let router = matrix_router();
    let cases = [
        ("owner", "needowner", true),
        ("owner", "needadmin", true),
        ("owner", "needmember", true),
        ("admin", "needowner", false),
        ("admin", "needadmin", true),
        ("admin", "needmember", true),
        ("member", "needowner", false),
        ("member", "needadmin", false),
        ("member", "needmember", true),
        ("ghost", "needowner", false),
        ("ghost", "needadmin", false),
        ("ghost", "needmember", false),
    ];
    for (user, command, allowed) in cases {
        let stage = resolve_for(&router, user, command).await;
        if allowed {
            assert_eq!(stage, Stage::Handled, "{user} 用 {command} 应当放行");
        } else {
            assert_eq!(
                stage,
                Stage::CommandRole,
                "{user} 用 {command} 应当被权限拦下"
            );
        }
    }
}

#[tokio::test]
async fn permission_denial_is_silent_unless_configured() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("secret")
            .role(RoleRequired::Owner)
            .on_sync(|_ctx: &Invocation| Ok(None)),
    );
    registry.register(
        "loud",
        CommandSpec::new("loud")
            .role(RoleRequired::Owner)
            .deny(Deny::say("只有主人能用"))
            .on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry).with_roles(Arc::new(
        StaticRoleResolver::new().with_user("member", RoleRequired::Member),
    ));
    let quiet = router.dispatch_detailed(event("/secret")).await;
    assert_eq!(quiet.stage, Stage::CommandRole);
    assert!(quiet.silent(), "默认权限拒绝要静默");
    let loud = router.dispatch_detailed(event("/loud")).await;
    assert_eq!(loud.stage, Stage::CommandRole);
    assert_eq!(loud.text().as_deref(), Some("只有主人能用"), "配了文案就要回话");
    assert_eq!(
        loud.reply.as_ref().map(|reply| reply.kind),
        Some(ReplyKind::Denied)
    );
}

#[tokio::test(start_paused = true)]
async fn cooldown_rejects_inside_window_and_opens_after() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping")
            .cooldown(Rate::per_window(1, 10))
            .on_sync(|_ctx: &Invocation| Ok(Some(Outgoing::new(ReplyKind::Handler, "pong")))),
    );
    let router = Router::new(registry);
    let first = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(first.stage, Stage::Handled);
    assert_eq!(first.text().as_deref(), Some("pong"));
    let second = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(second.stage, Stage::Cooldown);
    assert_eq!(second.text().as_deref(), Some("⏳ 触发太频繁了，10 秒后再试"));
    tokio::time::advance(Duration::from_secs(11)).await;
    let third = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(third.stage, Stage::Handled, "窗口过期后应当放行");
    assert_eq!(third.text().as_deref(), Some("pong"));
}

fn layered_router() -> (Router, Arc<AccessBook>, Arc<BanList>) {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping")
            .cooldown(Rate::per_window(1, 60))
            .on_sync(|_ctx: &Invocation| Ok(Some(Outgoing::new(ReplyKind::Handler, "pong")))),
    );
    let access = Arc::new(AccessBook::new());
    let ban = Arc::new(BanList::from_entries([
        BanEntry::new("u1").with_message("您的账号因违反Bot使用协议已被封禁")
    ]));
    let router = Router::new(Arc::clone(&registry))
        .with_access(Arc::clone(&access))
        .with_ban(Arc::clone(&ban))
        .with_limiter(Arc::new(RateLimiter::new()));
    (router, access, ban)
}

#[tokio::test]
async fn middleware_order_global_maintenance_wins() {
    let (router, _access, _ban) = layered_router();
    router.set_global_maintenance(Maintenance::on("全局维护中"));
    let outcome = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(
        outcome.stage,
        Stage::GlobalMaintenance,
        "全局维护要拦在第一级"
    );
    assert_eq!(outcome.text().as_deref(), Some("全局维护中"));
    assert!(
        outcome.reply.as_ref().map(|reply| reply.no_footer) == Some(true),
        "维护话术不带底部按钮"
    );
}

#[tokio::test]
async fn middleware_order_ban_beats_maintenance_and_cooldown() {
    let (router, access, _ban) = layered_router();
    access.set(
        "demo",
        AccessConfig::default().plugin_maintenance("插件维护中"),
    );
    let first = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(first.stage, Stage::Banned, "封禁要排在维护与冷却之前");
    assert_eq!(first.text().as_deref(), Some("您的账号因违反Bot使用协议已被封禁"));
    let second = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(
        second.stage,
        Stage::PluginMaintenance,
        "封禁话术被节流后, 才轮到插件维护"
    );
    assert_eq!(second.text().as_deref(), Some("插件维护中"));
}

#[tokio::test]
async fn middleware_order_plugin_then_command_maintenance() {
    let (router, access, _ban) = layered_router();
    router.ban().delete("u1").expect("先把封禁摘掉");
    access.set(
        "demo",
        AccessConfig::default()
            .plugin_maintenance("插件维护中")
            .command_rule(
                "ping",
                AccessRule {
                    maintenance: Maintenance::on("指令维护中"),
                    ..AccessRule::default()
                },
            ),
    );
    let outcome = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(
        outcome.stage,
        Stage::PluginMaintenance,
        "插件级维护要排在指令级之前"
    );
    assert_eq!(outcome.text().as_deref(), Some("插件维护中"));
    access.set(
        "demo",
        AccessConfig::default().command_rule(
            "ping",
            AccessRule {
                maintenance: Maintenance::on("指令维护中"),
                ..AccessRule::default()
            },
        ),
    );
    let outcome = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(
        outcome.stage,
        Stage::CommandMaintenance,
        "插件级关掉后才轮到指令级"
    );
    assert_eq!(outcome.text().as_deref(), Some("指令维护中"));
}

#[tokio::test]
async fn middleware_order_access_list_beats_cooldown() {
    let (router, access, _ban) = layered_router();
    router.ban().delete("u1").expect("先把封禁摘掉");
    access.set(
        "demo",
        AccessConfig {
            default: AccessRule {
                mode: AccessMode::Whitelist,
                deny: Deny::say("你没有权限使用该指令"),
                ..AccessRule::default()
            },
            ..AccessConfig::default()
        },
    );
    let first = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(first.stage, Stage::AccessList, "名单要排在冷却之前");
    assert_eq!(first.text().as_deref(), Some("你没有权限使用该指令"));
    assert_eq!(
        first.reply.as_ref().map(|reply| reply.kind),
        Some(ReplyKind::Denied)
    );
}

#[tokio::test]
async fn middleware_order_denied_silently_by_default() {
    let (router, access, _ban) = layered_router();
    router.ban().delete("u1").expect("先把封禁摘掉");
    access.set(
        "demo",
        AccessConfig {
            default: AccessRule {
                mode: AccessMode::Blacklist,
                users: vec!["u1".to_string()],
                ..AccessRule::default()
            },
            ..AccessConfig::default()
        },
    );
    let outcome = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(outcome.stage, Stage::AccessList);
    assert!(outcome.silent(), "名单默认静默");
}

#[tokio::test]
async fn middleware_order_cooldown_after_access_list() {
    let (router, access, _ban) = layered_router();
    router.ban().delete("u1").expect("先把封禁摘掉");
    access.set("demo", AccessConfig::default());
    let first = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(first.stage, Stage::Handled);
    let second = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(second.stage, Stage::Cooldown, "全放行之后才轮到冷却");
}

#[tokio::test]
async fn plugin_disabled_is_silent() {
    let (router, access, _ban) = layered_router();
    access.set(
        "demo",
        AccessConfig {
            disabled: true,
            ..AccessConfig::default()
        },
    );
    let outcome = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(outcome.stage, Stage::PluginDisabled);
    assert!(outcome.silent(), "停用的插件要静默");
}

#[tokio::test]
async fn private_scene_honours_disable_private() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("quiet")
            .disable_private()
            .on_sync(|_ctx: &Invocation| Ok(None)),
    );
    let router = Router::new(registry);
    let c2c = Incoming::new(Scene::C2c, Target::new(Scene::C2c, "u1"), "u1", "/quiet");
    let outcome = router.dispatch_detailed(c2c).await;
    assert_eq!(outcome.stage, Stage::PrivateBlocked);
    assert!(outcome.silent());
    let group = router.dispatch_detailed(event("/quiet")).await;
    assert_eq!(group.stage, Stage::Handled);
}

#[tokio::test]
async fn command_without_handler_is_silent() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register("demo", CommandSpec::new("stub").describe("还没写"));
    let router = Router::new(registry);
    let outcome = router.dispatch_detailed(event("/stub")).await;
    assert_eq!(
        outcome.stage,
        Stage::HandlerMissing,
        "没有处理器的指令要停在第十一级"
    );
    assert!(outcome.silent(), "没有处理器时静默");
}

#[tokio::test]
async fn handler_error_becomes_a_reply() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("boom").on_sync(|_ctx: &Invocation| Err(PluginError::handler("炸了"))),
    );
    let router = Router::new(registry);
    let outcome = router.dispatch_detailed(event("/boom")).await;
    assert_eq!(outcome.stage, Stage::Handled);
    let reply = outcome.reply.expect("错误也要有出口");
    assert!(!reply.body().trim().is_empty(), "错误出口不能是空串");
    assert!(reply.body().contains("炸了"));
    assert_eq!(reply.kind, ReplyKind::Handler);
}

#[tokio::test]
async fn maintenance_and_disable_use_core_defaults() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping").on_sync(|_: &Invocation| Ok(None)),
    );
    let access = Arc::new(AccessBook::new());
    access.set(
        "demo",
        AccessConfig {
            maintenance: Maintenance {
                enabled: true,
                message: String::new(),
            },
            ..AccessConfig::default()
        },
    );
    let router = Router::new(registry).with_access(access);
    let outcome = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(outcome.stage, Stage::PluginMaintenance);
    assert_eq!(
        outcome.text().as_deref(),
        Some(aurorix_core::DEFAULT_MAINTENANCE_MESSAGE),
        "空文案要落到 core 的默认维护话术"
    );
}

#[tokio::test]
async fn admin_is_exempt_from_maintenance() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping").on_sync(|_: &Invocation| Ok(None)),
    );
    let access = Arc::new(AccessBook::new());
    access.set(
        "demo",
        AccessConfig::default().plugin_maintenance("插件维护中"),
    );
    let router = Router::new(registry)
        .with_access(access)
        .with_roles(Arc::new(
            StaticRoleResolver::new().with_user("u1", RoleRequired::Admin),
        ));
    let outcome = router.dispatch_detailed(event("/ping")).await;
    assert_eq!(outcome.stage, Stage::Handled, "管理员不该被插件维护拦住");
}

#[tokio::test]
async fn maintenance_answers_commands_only() {
    let router = echo_router();
    router.set_global_maintenance(Maintenance::on("维护中，请稍后再试"));
    let chatter = router.dispatch_detailed(event("今天吃什么")).await;
    assert_eq!(
        chatter.stage,
        Stage::ResolveRoot,
        "闲聊不是指令, 不该进维护分支"
    );
    assert!(chatter.silent(), "闲聊不能被维护话术刷屏");
    let command = router.dispatch_detailed(event("/echo hi")).await;
    assert_eq!(command.stage, Stage::GlobalMaintenance);
    assert!(
        command.reply.is_some(),
        "指令在维护期要收到一句文案, 不能装没听见"
    );
}

#[tokio::test]
async fn owner_bypasses_disabled_plugin() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping").on_sync(|_: &Invocation| Ok(None)),
    );
    let access = Arc::new(AccessBook::new());
    access.set(
        "demo",
        AccessConfig {
            disabled: true,
            ..AccessConfig::default()
        },
    );
    let member = Router::new(Arc::clone(&registry)).with_access(Arc::clone(&access));
    assert_eq!(
        member.dispatch_detailed(event("/ping")).await.stage,
        Stage::PluginDisabled,
        "普通成员要被插件关闭拦住"
    );
    let owner = Router::new(registry)
        .with_access(access)
        .with_roles(Arc::new(
            StaticRoleResolver::new().with_user("u1", RoleRequired::Owner),
        ));
    assert_eq!(
        owner.dispatch_detailed(event("/ping")).await.stage,
        Stage::Handled,
        "Owner 不受插件关闭限制"
    );
}

#[tokio::test]
async fn owner_bypasses_admin_only_command_role() {
    let registry = Arc::new(CommandRegistry::new());
    registry.register(
        "demo",
        CommandSpec::new("ping")
            .role(RoleRequired::Admin)
            .on_sync(|_: &Invocation| Ok(None)),
    );
    let router = Router::new(registry).with_roles(Arc::new(
        StaticRoleResolver::new().with_user("u1", RoleRequired::Owner),
    ));
    assert_eq!(
        router.dispatch_detailed(event("/ping")).await.stage,
        Stage::Handled,
        "Owner 能跑管理员指令"
    );
}
