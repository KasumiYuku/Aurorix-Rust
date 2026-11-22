# 管理台 API

管理台和机器人在同一个进程里，浏览器打开 `http://127.0.0.1:8080/` 就是图形界面。
脚本要自动化就调 `/admin/api/*`，请求与响应都是 JSON；除登录外每个请求都要带会话 token。

## 登录

```bash
# 登录, token 会写进 admin.txt 里的 Cookie
curl -sS -c admin.txt -H 'Content-Type: application/json' \
  -d '{"password":"your-admin-password"}' \
  http://127.0.0.1:8080/admin/api/login
# → {"ok":true,"token":"<43 位 base64url>","expires_in":86400}
```

请求体只有一个 `password`，成功时响应带 `token`，同时也下发 Cookie。
之后每个请求带 `Authorization: Bearer <token>`，或者干脆用 `curl -b admin.txt` 复用 Cookie `px_admin`。
会话存在内存里，进程重启即失效；`remember: true` 把有效期从 24 小时拉到 30 天。

## 常用接口

| 方法 + 路径（省略 `/admin/api` 前缀） | 干什么 |
| --- | --- |
| `GET /overview`、`GET /stats` | 概览（版本、运行时长、网关状态）与收发统计 |
| `GET /logs`、`GET /stream` | 日志查询、日志 SSE 推流 |
| `GET /config`、`PUT /config` | 读配置返回字段表（标签、分组、热改标记）与当前值；写为稀疏补丁，只接受可写键 |
| `GET /bans`、`POST /bans`、`DELETE /bans/{openid}` | 封禁增删查 |
| `GET /assets`、`PUT /assets` | 图床 Provider 与白名单读写 |
| `GET /inbox/conversations`、`GET /inbox/messages` | 收件箱会话列表与消息分页 |
| `POST /inbox/messages` | 以机器人身份发一条消息 |
| `GET /jobs`、`POST /jobs/{id}/pause` | 定时任务列表、暂停 / 恢复 |
| `PUT /plugins/{id}` | 保存插件配置，请求体是自由 JSON |
| `POST /system/restart`、`POST /system/stop` | 重启 / 停止进程 |

更多端点（指令面板、档案、插件访问控制等）见管理台界面里的网络面板。

## 发一条消息

```bash
# conv 是收件箱里的会话 id
curl -sS -b admin.txt -H 'Content-Type: application/json' \
  -d '{"conv":"8F2C1D9E0A","text":"维护完成, 十分钟后恢复"}' \
  http://127.0.0.1:8080/admin/api/inbox/messages
# → {"ok":true}
```

消息经机器人真实发出，同时写进收件箱，管理台里马上能看到。
会话不存在返回 404 `{"error":"会话不存在"}`；机器人已断开返回 503 `{"error":"机器人未连接"}`。
上游发送失败返回 502，响应里的 `error` 就是上游给的原因。

## 看任务

```bash
# last_fire / next_fire 都是秒级时间戳
curl -sS -b admin.txt http://127.0.0.1:8080/admin/api/jobs
# → [{"id":"daily-report","kind":"cron",
#      "cron":"0 9 * * *","paused":false,
#      "last_fire":1730000000,"next_fire":1730086800}]
```

`kind` 是 `cron` 或 `interval`，`paused` 为 `true` 表示已暂停。
暂停 / 恢复用 `POST /admin/api/jobs/{id}/pause`，请求体 `{"paused":true}` 暂停，`{"paused":false}` 恢复。
任务 id 不存在返回 404 `{"error":"任务不存在"}`。

## 约定

- 出错统一 `{"error":"中文说明"}`，配状态码看：400 参数不对、401 未登录、404 不存在、502 上游失败、503 不可用。
- 未登录或 token 过期返回 401；未知的 `/admin/api/*` 返回 JSON 404，不回落到 HTML；静态资源长缓存（`max-age=31536000`），`index.html` 不缓存。

## 改前端
前端源码在 `webui/`，`pnpm install` 后 `pnpm build` 输出到 `crates/aurorix-admin/assets/dist`，构建二进制时自动嵌入，改完重新编译即可。
