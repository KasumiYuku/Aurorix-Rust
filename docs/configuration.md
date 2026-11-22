# 配置

配置文件是 `config.json`（JSON，不是 TOML），默认读工作目录；`AURORIX_CONFIG` 可以指定其它路径。
环境变量优先于文件，文件优先于内置默认值；改完重启生效。

## 一份能跑的配置

```json
{
  "port": 8080,
  "listen_addr": "0.0.0.0",
  "appid": "你的 AppID",
  "secret": "你的 AppSecret",
  "protocol": "websocket",
  "database": "postgres://用户:密码@127.0.0.1:5432/aurorix?sslmode=disable",
  "admin_password": "管理台密码",
  "log_level": "info",
  "render_pool": 4,
  "intents": ["GROUP_AT_MESSAGE_CREATE", "C2C_MESSAGE_CREATE"],
  "prefixes": ["/", "#", ""],
  "footer_buttons": [
    "帮助 | command | /help",
    "点赞 | counter | 已累计 {{count}} 次"
  ],
  "plugin_access": { "example-rating": { "default": { "mode": "whitelist", "users": ["123456789"] } } }
}
```

## 逐项说明

| 字段 | 说明 |
|---|---|
| `appid` / `secret` | QQ 开放平台机器人凭据，必填。 |
| `protocol` / `port` / `listen_addr` | 接入方式 `websocket` 或 `webhook`；监听端口与地址，`listen_addr` 只写 IP 不带端口。 |
| `database` | PostgreSQL 连接串，表结构在启动时自动创建。 |
| `admin_password` | 管理台密码，留空则不启用管理台。 |
| `log_level` | `debug` / `info` / `warn` / `error`。 |
| `render_pool` | 渲染线程数，默认 `4`。 |
| `intents` | 订阅的事件名，按需增删。 |
| `prefixes` | 指令前缀，空串表示无前缀也能识别。 |
| `footer_buttons` | 全局页脚按钮，写法见下。 |
| `plugin_access` | 插件与指令级访问控制。 |

其余字段保持默认即可，欢迎语、图片尺寸、模板覆盖目录等见 `config.example.json`。

## 环境变量

| 变量 | 覆盖字段 |
|---|---|
| `AURORIX_CONFIG` | 配置文件路径 |
| `AURORIX_APPID` | `appid` |
| `AURORIX_SECRET` | `secret` |
| `AURORIX_DATABASE` | `database` |
| `AURORIX_ADMIN_PASSWORD` | `admin_password` |
| `AURORIX_PROTOCOL` | `protocol` |
| `AURORIX_PORT` | `port` |

## 页脚按钮

```json
{
  "footer_buttons": [
    "帮助 | command | /help;style=gray",
    "点赞 | counter | 已累计 {{count}} 次;key=like;dedup=group",
    "开黑 | link | https://example.com/party"
  ]
}
```

基本用法是一行一个按钮，四段用 `|` 分隔：标签 / 类型 / 参数 / 选项。
选项写成 `;k=v`，也可以内联进参数段。
全部页脚合计最多 5 个按钮，超出的会被忽略。
样式取 `blue` / `gray` / `0`~`4`。

## 访问控制

```json
{
  "plugin_access": {
    "example-rating": {
      "default": { "mode": "whitelist", "users": ["123456789"] },
      "commands": { "/rating": { "mode": "blacklist", "users": ["987654321"] } }
    }
  }
}
```

`mode` 取 `off` / `whitelist` / `blacklist`，`users` 按 `mode` 解释成白名单或黑名单。
`role` 取 `owner` / `admin` / `member`，表示可用的最低角色。
不带 `commands` 时规则作用于整个插件。
