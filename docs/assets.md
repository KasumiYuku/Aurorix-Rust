# 图床

图片上传交给一组图床按 `priority` 从高到低降级尝试，任一成功即返回直链。
全部失败才报错，调用方保留原地址。
配置存在数据库里，由管理台维护，改完即时生效。

## 内置图床

| 名称 | 适用场景 |
|---|---|
| `qqbot` | 官方富媒体，免配置，仅群聊 / 私聊，直链只在该会话内有效 |
| `bilibili` | B 站相册，需要 `sessdata` 与 `biliJct` |
| `mihoyo` | 米游社，需要含 `stuid` / `stoken` / `ltoken` 的 cookie |
| `kurobbs` | 库街区，需要 `token` |
| `cnb` | CNB 仓库，需要 `token` 与 `defaultRepo` |
| `sgame` | 腾讯云 COS，需要 `secretId` 与 `secretKey` |
| `qzone` | QQ 空间相册，需要 cookie 与 `pSkey` |
| `qqgame` | QQ 游戏 COS，几乎免配置 |

共 13 个，其余同理，`name` 就是配置里的取值。
`qqbot` 不写进配置也会被追加到链尾，所以零配置也能上传。
想拿到长期可用的直链，优先配 `cnb` 或 `sgame`。

## 选择顺序

1. 按 `priority` 从高到低，同级保持配置里的先后。
2. 跳过 `enabled: false` 的条目。
3. 频道场景跳过只在群聊 / 私聊里有效的官方图床。
4. 单个失败换下一个，全部失败报 `all providers failed`。
5. 命中放行前缀（`assets:whitelist`）的地址原样返回，不上传。

## 配置一个图床

最小条目只要 `name` 与 `config`，`enabled`、`priority` 可以省。

```json
{
  "providers": [
    { "name": "cnb", "enabled": true, "priority": 20,
      "config": { "token": "你的 PAT", "defaultRepo": "user/repo" } },
    { "name": "mihoyo", "enabled": true, "priority": 10,
      "config": { "cookie": "stuid=...; stoken=...; ltoken=..." } },
    { "name": "kurobbs", "enabled": false, "priority": 5,
      "config": { "token": "..." } }
  ]
}
```

在管理台「图床」页编辑这份列表，保存即写入数据库。
脚本里用 `PUT /admin/api/assets` 提交同样的结构。
`priority` 数字越大越先尝试。
`config` 里的键名区分大小写。

## 出问题时

- 连续失败会熔断，冷却 60 秒后自动重试。
- 直链 404 或 403，先查 cookie / token 是否过期。
- 频道里只能用第三方图床，`qqbot` 在频道不可用。
- 超过 20 MiB 的图片直接拒。

报错里带图床名字与原因，定位到具体条目后改它的 `config` 即可。
