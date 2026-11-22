# 插件
插件是一个 Rust crate，只写指令、参数、权限和业务逻辑；前缀匹配、鉴权、限流、渲染上传、页脚合并、发送都由框架完成。

## 写一个最简指令
一个插件由若干 `PluginCommands` 组成，每个指令挂一个异步处理器。
```rust
use aurorix_plugin::{CommandSpec, Outgoing, PluginCommands, ReplyKind};

PluginCommands::new("hello", "问候")
    .describe("示例插件: 打招呼")
    .command(CommandSpec::new("/hello").describe("打个招呼").on(|_inv| async move {
        Ok(Some(Outgoing::new(ReplyKind::Handler, "你好")))
    }))
```
指令名带 `/`，注册时去掉一个前缀符号，规范路径就是 `hello`；别名用 `.alias("hi")`，几个名字命中同一处理器。

## 取参数
用 `ArgsSpec` 声明位置参数和选项，处理器里按名字取值。
```rust
use aurorix_plugin::{ArgsSpec, CommandSpec, FlagSpec, Outgoing, PositionalSpec, ReplyKind};

CommandSpec::new("/b30")
    .with_args(ArgsSpec::new()
        .positional(PositionalSpec::optional("user"))
        .flag(FlagSpec::value("top").short('n')))
    .on(|inv| async move {
        let user = inv.args.value("user").unwrap_or("me");
        let top = inv.args.value("top").unwrap_or("10");
        Ok(Some(Outgoing::new(ReplyKind::Handler, format!("{user} 的前 {top} 首"))))
    })
```
`/b30 花海 -n 10` 里 `花海` 进 `user`、`10` 进 `top`；没给时 `value` 返回空，用 `unwrap_or` 兜底。

## 回复文字 / 图片 / 按钮
一条回复可以同时带文字、卡片和按钮。
```rust
use aurorix_buttons::Builder;
use aurorix_plugin::{Outgoing, RenderCard, RenderRequest, ReplyKind};

Outgoing::new(ReplyKind::Handler, "文字回复").at(true);
Outgoing::new(ReplyKind::Handler, "卡片").render(RenderCard::new(
    RenderRequest::new(900, 520), "card.png", |kit| draw_card(kit)));
let board = Builder::new().scope("rating").command("again", "再查一次", "/b30", vec![]).build();
Outgoing::new(ReplyKind::Handler, "点一下").keyboard(board.unwrap());
```
`.render` 的第三个参数是绘制回调，渲染、编码、上传都由框架完成；按钮由 `aurorix_buttons::Builder` 拼装，`scope` 填插件 id，一行一个按钮。

## 在指令里读写数据
KV、插件库和素材都挂在 `Invocation` 上，直接 `.await`，不用自己起运行时。
```rust
CommandSpec::new("/b30").on(|inv| async move {
    let kv = inv.kv().unwrap(); //插件 KV, 未装配时为空
    let best = kv.get::<String>("best").await.unwrap_or_default(); //读回上次的值
    kv.set("best", &"13.42").await.unwrap(); //写入, 类型随值推断
    let hits = kv.incr("hits", 1).await.unwrap(); //自增并返回新值
    let rows = inv.db().unwrap().query("UPDATE score SET seen = true").await.unwrap(); //执行 SQL
    let cover = inv.assets().unwrap().read("covers/star.png").unwrap(); //读素材字节
    let text = format!("{} {hits} {rows} {}", best.unwrap_or_default(), cover.len());
    Ok(Some(Outgoing::new(ReplyKind::Handler, text)))
})
```
## 权限与冷却
角色和冷却都是 `CommandSpec` 上的一个方法。
```rust
use aurorix_core::RoleRequired;
use aurorix_plugin::{CommandSpec, Outgoing, Rate, ReplyKind};

CommandSpec::new("/set")
    .role(RoleRequired::Admin)
    .cooldown(Rate::per_window(2, 10))
    .on(|_inv| async move { Ok(Some(Outgoing::new(ReplyKind::Handler, "已更新"))) })
```
角色三档 `Member` / `Admin` / `Owner`，`Admin` 接受 admin 与 owner；冷却维度是 指令 + 会话 + 用户，`per_window(2, 10)` 即 10 秒内最多 2 次。
## 定时任务
用 `JobSpec` 声明调度、`async_job` 写逻辑，交给 `JobRegistry::register`。
```rust
use aurorix_plugin::{async_job, JobRegistry, JobSpec, Outgoing, ReplyKind, Scene, Target};

let spec = JobSpec::cron("daily", "my-plugin", "0 9 * * *")
    .target(Target::new(Scene::Group, "群 openid"));
jobs.register(spec, async_job(|run| async move {
    let text = format!("{} 的早报", run.plugin_id);
    Ok(Some(Outgoing::new(ReplyKind::Handler, text)))
}))
```
cron 是 5 段（分 时 日 月 周）；handler 返回 `Some(Outgoing)` 时框架自动发到 `target`，返回 `Ok(None)` 则这次不发。

## 装配
插件 crate 放 `plugins/<id>`，在 `src/main.rs` 的 `plugins()` 里加一行即被收集；素材放 `assets/<id>/`，模板放 `assets/<id>/templates/`，框架按插件 id 注册命名空间。
指令名与别名冲突只记警告，不阻断启动。
