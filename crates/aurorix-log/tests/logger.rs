use std::collections::BTreeMap;
use std::time::Duration;

use aurorix_log::{
    with_trace, Level, LogRecord, Logger, Options, Ring, DEFAULT_RING_BYTES, DEFAULT_RING_CAPACITY,
    MAX_MESSAGE_BYTES, TRUNCATION_MARK,
};

fn silent_logger(level: Level) -> std::sync::Arc<Logger> {
    Logger::new(Options::default().with_level(level).with_console(false))
}

fn small(level: Level, msg: &str) -> LogRecord {
    LogRecord::new(level, "gateway", msg, BTreeMap::new())
}

#[test]
fn ring_keeps_at_most_2048_entries_and_counts_evictions() {
    let ring = Ring::default();
    assert_eq!(
        ring.capacity(),
        DEFAULT_RING_CAPACITY,
        "默认条数上限应为 2048"
    );
    assert_eq!(
        ring.byte_budget(),
        DEFAULT_RING_BYTES,
        "默认字节预算应为 8 MiB"
    );
    for index in 0..DEFAULT_RING_CAPACITY + 52 {
        ring.push(small(Level::Info, &format!("第 {index} 条")));
    }
    let stats = ring.stats();
    assert_eq!(
        stats.entries, DEFAULT_RING_CAPACITY,
        "超出条数上限需淘汰到 2048"
    );
    assert_eq!(stats.dropped, 52, "淘汰计数需精确等于超出的条数");
    assert_eq!(
        stats.total,
        (DEFAULT_RING_CAPACITY + 52) as u64,
        "写入总数需累计"
    );
    let newest = ring.iter_newest_first(1);
    assert_eq!(
        newest[0].msg,
        format!("第 {} 条", DEFAULT_RING_CAPACITY + 51),
        "最新一条需保留"
    );
}

#[test]
fn ring_evicts_oldest_when_byte_budget_is_exceeded() {
    let logger = silent_logger(Level::Info);
    let big = "大".repeat(3000);
    let written = 1100;
    for index in 0..written {
        logger.info("gateway", &format!("{index}{big}"));
    }
    let stats = logger.stats();
    assert!(stats.bytes <= DEFAULT_RING_BYTES, "字节占用不得超过 8 MiB");
    assert!(stats.entries < written, "字节预算应淘汰最旧的记录");
    assert_eq!(
        stats.dropped,
        (written - stats.entries) as u64,
        "淘汰计数需与剩余条数互补"
    );
    assert!(stats.dropped > 0, "写入大记录后必须出现淘汰");
}

#[test]
fn ring_defaults_when_capacity_is_zero() {
    let ring = Ring::new(0, 0);
    assert_eq!(
        ring.capacity(),
        DEFAULT_RING_CAPACITY,
        "容量 0 需回落到 2048"
    );
    assert_eq!(
        ring.byte_budget(),
        DEFAULT_RING_BYTES,
        "预算 0 需回落到 8 MiB"
    );
    let custom = Ring::new(8, 4096);
    assert_eq!(custom.capacity(), 8, "自定义容量需生效");
    assert_eq!(custom.byte_budget(), 4096, "自定义预算需生效");
}

#[test]
fn message_over_8kib_is_truncated_with_marker() {
    let logger = silent_logger(Level::Info);
    let long = "长".repeat(9000);
    logger.info("overlong", &long);
    let records = logger.recent(1);
    let msg = &records[0].msg;
    assert!(msg.ends_with(TRUNCATION_MARK), "超长消息需带截断标记");
    let body = msg.trim_end_matches(TRUNCATION_MARK);
    assert!(body.len() <= MAX_MESSAGE_BYTES, "正文需不超过 8 KiB");
    assert_eq!(body.chars().count() * 3, body.len(), "截断需落在字符边界");
}

#[test]
fn level_filter_drops_records_below_threshold() {
    let logger = silent_logger(Level::Warn);
    logger.debug("gateway", "调试");
    logger.info("gateway", "信息");
    assert_eq!(logger.stats().entries, 0, "低于阈值不应进缓冲");
    logger.warn("gateway", "警告");
    logger.error("gateway", "错误");
    assert_eq!(logger.stats().entries, 2, "达到阈值需写入缓冲");
    logger.set_level(Level::Debug);
    logger.debug("gateway", "调试");
    assert_eq!(logger.stats().entries, 3, "放宽阈值后 debug 需写入");
}

#[test]
fn subscription_receives_broadcast_records() {
    let logger = silent_logger(Level::Info);
    let mut receiver = logger.subscribe();
    assert_eq!(logger.subscribers(), 1, "订阅后需有一个订阅者");
    logger.info("gateway", "推送一条");
    let record = logger.recv(&mut receiver).expect("订阅者应收到记录");
    assert_eq!(record.msg, "推送一条", "广播内容需一致");
    assert_eq!(record.module, "gateway", "广播内容需带模块名");
    assert_eq!(logger.lagged(), 0, "未落后时不应有丢弃");
}

#[test]
fn slow_subscriber_does_not_block_writer_and_lagged_grows() {
    let logger = silent_logger(Level::Info);
    let mut receiver = logger.subscribe();
    let total = aurorix_log::SUBSCRIBER_CAPACITY + 200;
    for index in 0..total {
        logger.info("gateway", &format!("第 {index} 条"));
    }
    assert_eq!(logger.stats().entries, total, "写入者不应被慢订阅者阻塞");
    let first = logger.recv(&mut receiver).expect("落后订阅者仍能读到记录");
    assert_eq!(
        first.msg,
        format!("第 {} 条", total - aurorix_log::SUBSCRIBER_CAPACITY),
        "落后时只保留最新 128 条"
    );
    assert_eq!(
        logger.lagged(),
        (total - aurorix_log::SUBSCRIBER_CAPACITY) as u64,
        "落后订阅者需累计丢弃计数"
    );
    let mut newest = first;
    while let Some(record) = logger.recv(&mut receiver) {
        newest = record;
    }
    assert_eq!(
        newest.msg,
        format!("第 {} 条", total - 1),
        "接收结束后最新一条需为最后写入"
    );
}

#[tokio::test]
async fn trace_id_propagates_across_await() {
    let logger = silent_logger(Level::Debug);
    let handle = logger.module("gateway");
    with_trace("m-tail-7", async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        handle.info("跨 await 的日志");
    })
    .await;
    let records = logger.recent(1);
    assert_eq!(
        records[0].trace.as_deref(),
        Some("m-tail-7"),
        "trace 需跨 await 传播"
    );
    logger.info("gateway", "作用域外");
    let records = logger.recent(1);
    assert_eq!(records[0].trace, None, "作用域外不应带 trace");
}

#[test]
fn trace_scope_does_not_leak_into_other_tasks() {
    let logger = silent_logger(Level::Debug);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("建运行时");
    runtime.block_on(async {
        let inner = logger.clone();
        let writer = tokio::spawn(async move {
            with_trace("t-1", async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                inner.info("gateway", "带 trace");
            })
            .await;
        });
        writer.await.expect("任务完成");
    });
    let records = logger.recent(1);
    assert_eq!(
        records[0].trace.as_deref(),
        Some("t-1"),
        "任务内 trace 需生效"
    );
    assert_eq!(
        aurorix_log::current_trace(),
        None,
        "任务结束后 trace 需清空"
    );
}

#[test]
fn iter_newest_first_returns_newest_first() {
    let logger = silent_logger(Level::Info);
    for index in 0..5 {
        logger.info("gateway", &format!("第 {index} 条"));
    }
    let newest = logger.recent(3);
    assert_eq!(newest.len(), 3, "limit 需生效");
    assert_eq!(newest[0].msg, "第 4 条", "第一条需为最新");
    assert_eq!(newest[1].msg, "第 3 条", "顺序需为倒序");
    assert_eq!(newest[2].msg, "第 2 条", "顺序需为倒序");
    assert_eq!(logger.recent(0).len(), 5, "limit 为 0 需返回全部");
    assert_eq!(logger.recent(99).len(), 5, "limit 超出需返回全部");
}

#[test]
fn record_ids_start_at_one_and_increase() {
    let logger = silent_logger(Level::Info);
    logger.info("gateway", "一");
    logger.info("gateway", "二");
    logger.info("gateway", "三");
    let mut records = logger.recent(3);
    assert_eq!(records[0].id, 3, "第三条编号需为 3");
    records.reverse();
    assert_eq!(records[0].id, 1, "编号需从 1 开始");
    assert_eq!(records[1].id, 2, "编号需自增 1");
}

#[test]
fn error_level_accumulates_error_counter() {
    let logger = silent_logger(Level::Info);
    logger.info("gateway", "普通");
    logger.warn("gateway", "警告");
    assert_eq!(logger.errors(), 0, "非 error 不计入错误数");
    logger.error("gateway", "失败");
    assert_eq!(logger.errors(), 1, "error 需计入错误数");
    assert_eq!(logger.total(), 3, "总数需包含全部级别");
}

#[test]
fn summary_counts_levels_and_modules() {
    let logger = silent_logger(Level::Debug);
    let gateway = logger.module("gateway");
    let storage = logger.module("storage");
    gateway.info("连接建立");
    gateway.error("连接断开");
    storage.debug("慢查询");
    let summary = logger.summary();
    assert_eq!(summary.levels.get("info"), Some(&1), "info 计数");
    assert_eq!(summary.levels.get("error"), Some(&1), "error 计数");
    assert_eq!(summary.levels.get("debug"), Some(&1), "debug 计数");
    assert_eq!(summary.scopes.get("gateway"), Some(&2), "模块计数");
    assert_eq!(summary.scopes.get("storage"), Some(&1), "模块计数");
}

#[test]
fn global_module_handle_uses_singleton() {
    let logger = aurorix_log::init(Options::default().with_level(Level::Debug));
    let handle = aurorix_log::module("gateway");
    assert_eq!(handle.name(), "gateway", "句柄需带模块名");
    handle.info("全局单例");
    let records = aurorix_log::recent(1);
    assert_eq!(records[0].msg, "全局单例", "句柄需写入全局单例");
    assert!(aurorix_log::stats().entries >= 1, "全局缓冲需有条目");
    assert_eq!(logger.level(), Level::Debug, "全局级别需与初始化一致");
    aurorix_log::set_level(Level::Info);
    assert_eq!(global_level(), Level::Info, "全局级别需可热更");
    aurorix_log::set_level(Level::Debug);
}

fn global_level() -> Level {
    aurorix_log::global().level()
}

#[test]
fn empty_module_and_fields_are_recorded() {
    let logger = silent_logger(Level::Info);
    logger.log(
        Level::Info,
        "  ",
        "带字段",
        BTreeMap::from([("openid".to_string(), "u1".to_string())]),
    );
    let records = logger.recent(1);
    assert_eq!(records[0].module, "system", "空模块名需回落到 system");
    assert_eq!(
        records[0].fields.get("openid").map(String::as_str),
        Some("u1"),
        "结构化字段需保留"
    );
}

#[test]
fn serialized_record_matches_go_field_names() {
    let record = small(Level::Warn, "机器可读");
    let encoded = serde_json::to_value(&record).expect("序列化");
    assert_eq!(encoded["level"], serde_json::json!("WARN"), "级别需大写");
    assert_eq!(
        encoded["scope"],
        serde_json::json!("gateway"),
        "模块名键需为 scope"
    );
    assert_eq!(
        encoded["msg"],
        serde_json::json!("机器可读"),
        "消息键需为 msg"
    );
    assert!(encoded.get("trace").is_none(), "空 trace 不应序列化");
    assert!(encoded.get("group").is_none(), "空 group 不应序列化");
    assert!(encoded.get("user").is_none(), "空 user 不应序列化");
    assert!(encoded.get("fields").is_none(), "空字段不应序列化");
    let decoded: LogRecord = serde_json::from_value(encoded).expect("反序列化");
    assert_eq!(decoded, record, "往返需一致");
}

#[test]
fn group_and_user_are_extracted_from_the_message() {
    let logger = silent_logger(Level::Info);
    let mut receiver = logger.subscribe();
    logger.info(
        "message",
        "被动回复窗口已失效 target=0 group=819EFED8D4D90696B758AE620B59546C user=618F87D017231E57C5B8DCDD07761AAB err=x",
    );
    let records = logger.recent(1);
    let record = &records[0];
    assert_eq!(
        record.group, "819EFED8D4D90696B758AE620B59546C",
        "缓冲需带 group"
    );
    assert_eq!(
        record.user, "618F87D017231E57C5B8DCDD07761AAB",
        "缓冲需带 user"
    );
    let encoded = serde_json::to_value(record).expect("序列化");
    assert_eq!(
        encoded["group"],
        serde_json::json!("819EFED8D4D90696B758AE620B59546C"),
        "序列化键需为 group"
    );
    assert_eq!(
        encoded["user"],
        serde_json::json!("618F87D017231E57C5B8DCDD07761AAB"),
        "序列化键需为 user"
    );
    let pushed = logger.recv(&mut receiver).expect("订阅者应收到记录");
    assert_eq!(pushed.group, record.group, "订阅侧也要能看到 group");
    assert_eq!(pushed.user, record.user, "订阅侧也要能看到 user");
    logger.info("message", "看起来像但不是 groups=1 users=2");
    let plain = logger.recent(1);
    assert_eq!(plain[0].group, "", "groups= 不应被当成 group=");
    assert_eq!(plain[0].user, "", "users= 不应被当成 user=");
    let encoded = serde_json::to_value(&plain[0]).expect("序列化");
    assert!(
        encoded.get("group").is_none() && encoded.get("user").is_none(),
        "空值不出现"
    );
}

#[test]
fn default_options_match_go_defaults() {
    let options = Options::default();
    assert_eq!(options.level, Level::Info, "默认级别需为 info");
    assert_eq!(
        options.capacity, DEFAULT_RING_CAPACITY,
        "默认条数上限需为 2048"
    );
    assert_eq!(
        options.byte_budget, DEFAULT_RING_BYTES,
        "默认字节预算需为 8 MiB"
    );
    assert!(options.console, "默认需输出控制台");
}

#[test]
fn console_output_can_be_disabled() {
    let quiet = silent_logger(Level::Info);
    quiet.info("gateway", "静默");
    assert_eq!(quiet.recent(1).len(), 1, "关闭控制台不影响缓冲");
    let loud = Logger::new(
        Options::default()
            .with_level(Level::Info)
            .with_console(true),
    );
    loud.info("gateway", "输出一行");
    assert_eq!(loud.recent(1).len(), 1, "开启控制台也要写入缓冲");
}
