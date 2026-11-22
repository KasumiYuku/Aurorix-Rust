# 存储

数据都存在 PostgreSQL 里，启动时自动建表，指到一个已经在跑的旧库可以直接接管，不用手工迁移。
插件日常只碰两件事：用 KV 存点小数据，用插件自己的表存结构化数据。

## 表

| 表 | 存什么 |
|---|---|
| `kv_data` | KV，值是 JSON，按作用域与命名空间分组 |
| `stats_daily` | 按天累计的接收、发送、按钮点击 |
| `stats_conversations` | 按会话累计的消息数、显示名、是否好友 |
| `inbox_conversations` | 收件箱会话索引：类型、名称、最后一条消息时间与预览 |
| `inbox_messages` | 收件箱消息：方向、发送者、内容、平台消息 id、时间 |
| `stats_migrations` | 统计回填的一次性进度标记 |
| `plugin_migrations` | 各插件已执行的迁移版本，按插件与版本去重 |

框架只建表、建索引，不删表、不删列，也不改既有列类型，所以旧库里的数据不会被碰。

## 读写 KV

每个插件有自己的 KV 命名空间，不用自己拼键名。

```rust
let Some(kv) = inv.kv() else {
    return Ok(None);
};

kv.set("last_user", &inv.user).await?;    // 写：能转成 JSON 的值都能存
let hits = kv.incr("hits", 1).await?;     // 整数自增，返回自增后的值
let saved = kv.get("last_user").await?;   // 读：键不存在得到空
let name = saved.unwrap_or_else(|| "访客".to_string());
Ok(Some(Outgoing::new(ReplyKind::Handler, format!("{name}，第 {hits} 次"))))
```

`incr` 是原子的，并发调用不会丢计数。

## 插件自己的表

`inv.db()` 是当前插件的数据库句柄，每个插件有独立的一条迁移版本记录。

```rust
let db = inv.db().ok_or(PluginError::handler("插件库未就绪"))?;
db.migrate(&[MigrationStep {
    version: 1,
    name: "create_score",
    sql: "CREATE TABLE IF NOT EXISTS score (uid TEXT PRIMARY KEY, score BIGINT NOT NULL DEFAULT 0)",
}]).await?;                                // 版本 1 只执行一次，重复启动会跳过
db.query("UPDATE score SET score = score + 1").await?;  // 执行写语句，返回值是影响行数
```

迁移按版本号升序执行，已应用的版本会跳过；表名由插件自己的 SQL 决定，框架不加前缀。

## 统计与收件箱

收发计数按天累计，落在 `stats_daily` 与 `stats_conversations`，管理台概览读的就是这两张表。
收件箱保存入站和出站两个方向的消息，会话列表按最后一条消息时间倒序。
管理台发出去的消息也会以出站方向落库，和机器人回复共用 `inbox_messages`。
写入走批量，攒够一批再刷库；批量失败会退回同步写，不影响已经发出的回复。

## 连接

`database` 字段写 DSN：`postgres://user:password@host:5432/dbname?sslmode=disable`。
连接池默认 16 条连接、常驻空闲 8 条。
启动时连不上库会直接退出，不会带病运行。
