# aurorix-assets

图床聚合与上传降级链，与 Go 原版 `lib/assets/**` 逐条对齐。

## 组成

| 模块 | 职责 |
|---|---|
| `host.rs` | `ImageHost` 聚合器：链路构造、whitelist 直通、本地/公网判定、候选筛选、失败切换、上传后 HEAD 探测、并发闸门、Markdown 改写 |
| `provider.rs` | `Provider` 契约、`UploadInput`/`UploadContext`/`ResolvedImage`、`MediaKind`/`MediaKinds` |
| `registry.rs` | `ProviderRegistry`、`ProviderEntry`、管理台 schema（`ProviderSpec`/`ConfigField`） |
| `providers/*.rs` | 13 个内置 provider（见下表） |
| `input.rs` | 输入归一化：path、`file://`、`data:` URL、裸 base64、公网 URL |
| `mime.rs` | 魔数嗅探（PNG/JPEG/GIF/WebP/BMP、mp4/webm/quicktime）与宽高探测 |
| `sign.rs` | `bkn`、COS `q-sign`（HMAC-SHA1）、`urlencode_go_compat`、MD5/SHA1/base64/UUID |
| `http.rs` | 注入式 HTTP 抽象（`HttpClient`）与 reqwest 实现、multipart 构造 |
| `official.rs` | 官方图床上传闭包的注入点（`set_official_upload`/`official_upload`） |
| `cache.rs` | 内容哈希去重缓存（增强项） |

## 内置 provider

`qqbot`（官方，默认启用、会话级、跳过探测）、`sgame`、`cnb`、`kurobbs`、`mihoyo`、`bilibili`、`qzone`、`qqstream`、`qqgame`、`announce`、`homework`、`chinaunicom`、`chatglm`。
其中 `cnb`、`kurobbs` 与 `qqbot` 默认跳过上传后的 HEAD 探测。

## 配置

```json
{
  "providers": [
    { "name": "sgame", "priority": 30, "config": { "secretId": "...", "secretKey": "..." } },
    { "name": "kurobbs", "priority": 10, "probe": false, "config": { "token": "..." } }
  ],
  "whitelist": ["https://img.example.com/"]
}
```

链路构造顺序与 Go 一致：过滤 `enabled == false` → 按 `priority` 降序稳定排序 → 逐个实例化（配置缺失即构造失败并被静默跳过）→ 「默认启用但未配置」的 provider 追加到链尾。

## 增强项：内容哈希去重缓存

Go 原版**没有**任何去重缓存：每次 `uploadTo` 都会重新上传。本 crate 增加了进程内的
`UploadDedupCache`（`cache.rs`，`ImageHost::dedup()`）：

* key = 图片字节的 **MD5 hex**，value = `已上传 URL + provider 名`；
* 容量固定、进程内 LRU，默认 512 条（`ImageHost::with_dedup_capacity` 可调，0 表示关闭），
  超出容量按最近最少使用逐条淘汰，不会无界增长；
* 只对 `MediaKind::Image` 且字节非空生效；
* **跳过会话级 provider**（`session_scoped()`，即 `qqbot`）：它的直链只在签发会话内短期有效，
  跨会话复用会拿到失效链接；
* 命中时计入 `HostStats::cache_hits`，不重复计入 `uploaded`，也不再触发上传后探测。

这是唯一一处有意偏离 Go 行为的地方，除此之外上传链、错误文案与 URL 拼接均与 Go 原版一致。

## 与 Go 原版的细微差异（已知且有意）

* `cnb` 的自动删除用 `tokio::spawn` 实现，语义同 Go 的 goroutine（睡 N 秒后 DELETE，404 视为成功）；
* `homework` 的 `data` 非对象时按「缺少图片 URL」处理（同 Go 的字节级判断）；
* HTTP 抓取（`fetch_remote`）与 HEAD 探测共用一个 reqwest 客户端，重定向上限 5，
  Go 的下载客户端默认 10、探测客户端 5。
