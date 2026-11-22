# Aurorix 文档

Aurorix 是 Rust 写的 QQ 开放平台机器人框架，单进程运行：接入、路由、出图、发送、入库都由框架做，插件只写指令和业务。
这里是文档索引，按需要下钻，不用从头读到尾。

## 导航

| 文档 | 什么时候读 |
|---|---|
| [配置项速查](configuration.md) | 要改 `config.json`、想知道某个字段的默认值 |
| [插件开发](plugins.md) | 写第一个指令、加参数、管权限、配冷却 |
| [模板](templates.md) | 想让卡片文案可改、要覆盖内置模板 |
| [渲染](render.md) | 要出一张图、换字体、调 GPU 开关 |
| [图床](assets.md) | 图片要传到图床、正在排查上传失败 |
| [存储](storage.md) | 要存数据、建自己的表、查统计数 |
| [管理台 API](admin-api.md) | 写脚本调 `/admin/api/*`、接自己的界面 |
| [架构](architecture.md) | 想知道一条消息在框架里走了哪几步 |
| [API 索引](api/README.md) | 想知道某个 crate 一共有哪些类型与函数（从源码生成，永远最新） |

## 配置格式

配置文件是 `config.json`，JSON 不是 TOML，字段与 Go 版实例兼容，旧文件可以直接用。
`AURORIX_CONFIG` 指定配置文件路径，不设时读工作目录下的 `config.json`。
环境变量可覆盖文件里的同名字段：`AURORIX_APPID`、`AURORIX_SECRET`、`AURORIX_DATABASE`、`AURORIX_ADMIN_PASSWORD`、`AURORIX_PROTOCOL`、`AURORIX_PORT`。

改完配置重启进程生效；管理台的 `/admin/api/config` 读写的是同一个文件。

## 最小配置

```json
{
  "port": 8080,
  "appid": "你的 AppID",
  "secret": "你的 AppSecret",
  "protocol": "websocket",
  "database": "postgres://user:pass@127.0.0.1:5432/aurorix?sslmode=disable",
  "admin_password": "自己定一个"
}
```

填好这几项就能启动，其余字段都有默认值，要调再看[配置项速查](configuration.md)。

## 起步

- 先有一个能连的 PostgreSQL 库，连接串填进 `database`。
- 复制 `config.example.json` 为 `config.json`，填 `appid`、`secret`、`database`、`admin_password`。
- `cargo run` 启动，浏览器打开 `http://127.0.0.1:8080/` 就是管理台。
- 在群里 @机器人 发一条 `/help`，能看到当前注册了哪些指令。

## 本地开发

```bash
# 提交前依次跑这四条
cargo fmt --all --check                     # 格式校验
cargo clippy --workspace --all-targets      # 静态检查
cargo test --workspace                      # 单元与集成测试
cargo run -p xtask -- check-comments        # 注释规范检查
```

需要真实 PostgreSQL 的用例由 `AURORIX_TEST_DATABASE` 开启，未设置时自动跳过。改了公开 API 后跑 `cargo run -p xtask -- gen-api` 刷新 [API 索引](api/README.md)，`--check` 可校验索引是否过期。

## 示例插件

`plugins/example-rating` 是一个能跑通的完整插件：参数、冷却、出图、按钮都有，照它抄比读文档快。
