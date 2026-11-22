# Aurorix-Rust

![Rust](https://img.shields.io/badge/Rust-1.89-blue)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-14%2B-blue)
![License](https://img.shields.io/badge/License-MIT-green)

用 Rust 构建的 QQ 开放平台机器人框架

## 特性

| 维度 | 能力 |
|---|---|
| 接入 | WebSocket 网关（Identify / Resume / 心跳 / 重连退避）与 Webhook 双模式，intents 显式订阅 |
| 消息 | 纯文本、Markdown、媒体消息与按钮键盘，群聊 / 单聊 / 频道 / 频道私信各自按场景发送；图片超阈值自动分片上传 |
| 指令 | `/` `#` `!` 前缀、别名、子指令、类型化参数、member / admin / owner 角色、访问控制、封禁、冷却限流、帮助 |
| 处理器 | 指令与定时任务处理器原生 async，纯同步逻辑用 `on_sync` / `sync_job` |
| 渲染 | 进程内 2D 渲染（skia-safe），PNG / WebP / JPEG 编码，专用线程池与字体、纹理缓存；`gpu` feature 开启硬件加速，运行时 `AURORIX_GPU=auto\|on\|off`，没有可用 GPU 时自动回落多线程光栅 |
| 图床 | 13 个内置 Provider，按优先级降级，连续失败熔断；`qqbot` 走平台官方直传，去重缓存 |
| 存储 | PostgreSQL：KV、统计、收件箱归档、插件独立库与迁移、批量写入、L1 缓存 |
| 定时任务 | `interval` 与 5 段 cron 调度，`JobRegistry` 注册任务，支持暂停、恢复与实时状态查询 |
| 管理台 | 内嵌 WebUI 图形界面，`/admin/api/*` 同源 JSON 接口，SSE 实时日志；发消息经 Bot 真实发送并落库 |

## 快速开始

依赖两项：Rust 1.89+ 与 PostgreSQL 14+。表结构在启动时幂等创建，不需要手工迁移。

```bash
git clone <repo-url> Aurorix && cd Aurorix
cp config.example.json config.json
cargo run --release
```

编辑 `config.json`，至少填 `appid`、`secret`、`database`、`admin_password` 四项。启动后浏览器打开 `http://127.0.0.1:8080/` 就是管理台图形界面——WebUI 已内嵌进二进制，无需单独部署前端；`/admin/api/*` 是同源的 JSON 接口，登录密码取 `admin_password`。

`protocol` 设为 `websocket` 时进程直连网关；设为 `webhook` 时进程监听回调路径，需要在 QQ 开放平台填写回调地址。任一字段都可以用环境变量覆盖，见[配置项速查](docs/configuration.md)。

常用字段：

| 字段 | 说明 |
|---|---|
| `appid` / `secret` | 机器人凭据，必填 |
| `database` | PostgreSQL DSN，必填 |
| `admin_password` | 管理台登录密码，必填 |
| `protocol` | `websocket` 或 `webhook`，缺省 `webhook` |
| `port` / `listen_addr` | 管理台端口与监听地址，端口缺省 `8080`，地址缺省按协议推导 |
| `intents` | 订阅的事件集合，按需裁剪 |
| `log_level` | 日志级别，缺省 `info` |
| `render_pool` | 渲染线程数，缺省 `4` |
| `data_dir` | 运行数据目录，缺省 `data` |
| `footer_buttons` | 页脚按钮列表，`footer_buttons_enabled` 与 `footer_buttons_group` / `_c2c` / `_channel` / `_dm` 按场景生效，点击由框架处理 |
| `plugin_settings` / `plugin_access` | 按插件 ID 分组的配置项与访问控制 |

## 写一个插件

插件是一个 Rust crate，放在 `plugins/<plugin_id>`，在 `src/main.rs` 的 `plugins()` 里加一行即可装配。指令处理器原生 async，可以直接 `.await` 框架提供的 KV、素材与渲染能力。

```rust
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
                    let kv = inv.kv().ok_or_else(|| PluginError::handler("插件库未就绪"))?;
                    let hits = kv.incr("hits", 1).await.map_err(fail)?;
                    let saved = kv.get::<String>("greeting").await.map_err(fail)?;
                    let greeting = saved.filter(|text| !text.is_empty()).unwrap_or_else(|| "你好".to_string());
                    let who = inv.username.as_deref().unwrap_or(&inv.user);
                    let text = format!("{greeting}, {who} · 第 {hits} 次");
                    Ok(Some(Outgoing::new(ReplyKind::Handler, text)))
                }),
        )
}

fn fail(err: impl std::fmt::Display) -> PluginError {
    PluginError::handler(err.to_string())
}
```

纯同步逻辑改用 `CommandSpec::on_sync`，无需包一层 async。参数、冷却、按钮、渲染卡、素材目录的完整写法见 [plugins/example-rating](plugins/example-rating) 与[插件开发](docs/plugins.md)。

`CommandSpec` 常用构建方法：

| 方法 | 作用 |
|---|---|
| `role(RoleRequired)` | 最低角色，取值 `Member` / `Admin` / `Owner` |
| `alias` / `aliases` | 追加别名，多个名字命中同一处理器 |
| `prefix` / `prefixes` | 追加触发前缀，缺省 `/` `#` `!` |
| `cooldown(Rate)` | 冷却，如 `Rate::per_window(2, 10)` 表示 10 秒内 2 次 |
| `with_args(ArgsSpec)` | 声明位置参数与 `--flag`，解析结果在 `inv.args` |
| `access` / `maintenance` / `deny` | 访问控制规则、维护期话术、拒绝话术 |
| `subcommand` / `fallback` | 子指令与兜底处理器 |
| `disable_private` / `no_footer` | 禁用私聊触发、本条关闭页脚按钮 |

`Outgoing` 常用构建方法：

| 方法 | 作用 |
|---|---|
| `Outgoing::new(ReplyKind, text)` | 基础回复，正常结果用 `ReplyKind::Handler` |
| `.at(true)` | 回复带上 @ 提及 |
| `.image(Attachment::png(bytes))` | 附带图片或媒体，另有 `jpeg` / `webp` |
| `.keyboard(Keyboard)` | 附带按钮键盘 |
| `.render(RenderCard)` | 附带进程内渲染出的卡片 |
| `.no_footer()` | 本条不追加页脚按钮 |

定时任务用同一个注册面：`JobSpec::interval(id, plugin_id, every)` 或 `JobSpec::cron(id, plugin_id, "0 9 * * *")`，配 `async_job` / `sync_job` 处理器，交给 `JobRegistry::register`，管理台与 `GET /admin/api/jobs` 读的是同一份实时状态。

## 目录结构

```txt
Aurorix/
├── crates/              框架：接入、指令、渲染、图床、存储、管理台
├── plugins/             业务插件 crate（如 example-rating）
├── webui/               管理台前端源码（pnpm workspace，构建产物嵌入二进制）
├── xtask/               仓库自检（注释规范、代码规模）
├── src/main.rs          本实例入口：装配插件、连接数据库、启动运行时
├── docs/                技术文档
├── config.json          运行期配置（config.example.json 为模板）
├── logs/                按本地日期轮转的日志文件
├── data/                本地运行数据（如指令面板快照）
├── templates/override/  模板覆盖目录，同名模板优先于内置与插件版本
└── assets/<plugin_id>/  插件素材：fonts/ 与 templates/
```

## 文档

| 文档 | 内容 |
|---|---|
| [配置项速查](docs/configuration.md) | 一份能跑的 `config.json`、字段说明、环境变量、页脚按钮与访问控制 |
| [插件开发](docs/plugins.md) | 六步写插件：最简指令、参数、回复、读写数据、权限冷却、定时任务 |
| [模板](docs/templates.md) | 变量填充、覆盖目录、内联按钮、图片尺寸、页脚按钮 |
| [渲染](docs/render.md) | 画卡片、常用绘制、字体、输出格式、GPU 开关 |
| [图床](docs/assets.md) | 内置图床、选择顺序、配置示例、排障 |
| [存储](docs/storage.md) | 表结构、KV 用法、插件自己的表与迁移 |
| [管理台 API](docs/admin-api.md) | 登录、常用端点、发消息、看任务 |
| [架构](docs/architecture.md) | 模块地图、一条消息的经历、出问题先看这里 |
| [API 索引](docs/api/README.md) | 全量公开类型与函数，按 crate 自动生成，改代码后跑 `gen-api` 刷新 |

## 许可

MIT，见 [LICENSE](LICENSE)。
