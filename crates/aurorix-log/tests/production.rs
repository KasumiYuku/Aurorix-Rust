use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_log::{
    Clock, FileOptions, FileSink, Level, LogRecord, Logger, Options, Ring, RingFilter,
    SlowThresholds, DEFAULT_QUERY_LIMIT, SLOW_MARK,
};
use chrono::NaiveDate;

struct FakeClock {
    today: Mutex<NaiveDate>,
}

impl FakeClock {
    fn new(year: i32, month: u32, day: u32) -> Arc<Self> {
        Arc::new(Self {
            today: Mutex::new(NaiveDate::from_ymd_opt(year, month, day).expect("合法日期")),
        })
    }

    fn set(&self, year: i32, month: u32, day: u32) {
        *self.today.lock().expect("时钟锁") =
            NaiveDate::from_ymd_opt(year, month, day).expect("合法日期");
    }
}

impl Clock for FakeClock {
    fn today(&self) -> NaiveDate {
        *self.today.lock().expect("时钟锁")
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("aurorix-log-{tag}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

fn read_lines(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => text.lines().map(str::to_string).collect(),
        Err(_) => Vec::new(),
    }
}

fn record(level: Level, module: &str, msg: &str) -> LogRecord {
    let mut record = LogRecord::new(level, module, msg, BTreeMap::new());
    record.time = 1_771_081_445_123;
    record
}

fn sink(dir: &Path, clock: Arc<FakeClock>, min_level: Level) -> FileSink {
    FileSink::open_with(
        FileOptions::new(dir)
            .with_prefix("app")
            .with_error_prefix("app-err")
            .with_min_level(min_level),
        clock,
    )
    .expect("打开日志文件")
}

#[test]
fn four_levels_split_across_main_and_error_files() {
    let dir = temp_dir("split");
    let clock = FakeClock::new(2026, 2, 14);
    let sink = sink(&dir, clock.clone(), Level::Debug);
    for (level, msg) in [
        (Level::Debug, "调试"),
        (Level::Info, "信息"),
        (Level::Warn, "警告"),
        (Level::Error, "错误"),
    ] {
        sink.write(&record(level, "gateway", msg)).expect("写入");
    }
    sink.flush().expect("刷新");
    let (main_path, error_path) = sink.current_paths();
    assert_eq!(main_path, dir.join("app-2026-02-14.log"));
    assert_eq!(error_path, dir.join("app-err-2026-02-14.log"));
    let main = read_lines(&main_path);
    let errors = read_lines(&error_path);
    assert_eq!(main.len(), 4, "全量日志需四行: {main:?}");
    assert_eq!(errors.len(), 2, "错误日志只收 WARN/ERROR: {errors:?}");
    assert!(
        main[0].contains(" | DEBUG | [gateway] | 调试"),
        "{}",
        main[0]
    );
    assert!(
        main[2].contains(" | WARN  | [gateway] | 警告"),
        "{}",
        main[2]
    );
    assert!(errors[0].contains(" | WARN  "), "{}", errors[0]);
    assert!(errors[1].contains(" | ERROR | "), "{}", errors[1]);
    let stats = sink.stats();
    assert_eq!(stats.lines, 4, "行数统计");
    assert_eq!(stats.error_lines, 2, "错误行数统计");
    assert_eq!(stats.write_errors, 0, "正常写入不应有错误");
}

#[test]
fn min_level_skips_low_levels_on_disk() {
    let dir = temp_dir("minlevel");
    let clock = FakeClock::new(2026, 2, 14);
    let sink = sink(&dir, clock.clone(), Level::Warn);
    sink.write(&record(Level::Debug, "gateway", "调试"))
        .expect("写入");
    sink.write(&record(Level::Info, "gateway", "信息"))
        .expect("写入");
    sink.write(&record(Level::Warn, "gateway", "警告"))
        .expect("写入");
    sink.flush().expect("刷新");
    let (main_path, error_path) = sink.current_paths();
    assert_eq!(read_lines(&main_path).len(), 1, "低于阈值不落盘");
    assert_eq!(read_lines(&error_path).len(), 1, "错误文件同样过滤");
}

#[test]
fn day_change_switches_to_a_new_file() {
    let dir = temp_dir("rotate");
    let clock = FakeClock::new(2026, 2, 14);
    let sink = sink(&dir, clock.clone(), Level::Debug);
    for index in 0..3 {
        sink.write(&record(Level::Info, "gateway", &format!("第一天 {index}")))
            .expect("写入");
    }
    sink.flush().expect("刷新");
    let first = dir.join("app-2026-02-14.log");
    assert_eq!(read_lines(&first).len(), 3, "第一天三行");
    clock.set(2026, 2, 15);
    sink.write(&record(Level::Info, "gateway", "第二天"))
        .expect("写入");
    sink.flush().expect("刷新");
    let (main_path, error_path) = sink.current_paths();
    assert_eq!(main_path, dir.join("app-2026-02-15.log"), "跨天需换新文件");
    assert_eq!(error_path, dir.join("app-err-2026-02-15.log"));
    assert_eq!(read_lines(&first).len(), 3, "旧文件不再追加");
    assert_eq!(read_lines(&main_path).len(), 1, "新文件从零开始");
    assert_eq!(sink.stats().rotations, 2, "打开一次 + 跨天一次");
    assert_eq!(
        sink.current_day(),
        Some(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap())
    );
}

#[test]
fn prune_removes_only_expired_matching_files() {
    let dir = temp_dir("prune");
    let clock = FakeClock::new(2026, 2, 14);
    let sink = FileSink::open_with(
        FileOptions::new(&dir)
            .with_prefix("app")
            .with_error_prefix("app-err")
            .with_retention_days(2),
        clock.clone(),
    )
    .expect("打开日志文件");
    for name in [
        "app-2026-02-10.log",
        "app-err-2026-02-10.log",
        "app-2026-02-12.log",
        "app-2026-02-13.log",
        "app-err-2026-02-13.log",
        "other-2026-02-10.log",
        "app-2026-02-10.txt",
        "app-unmatched.log",
    ] {
        std::fs::write(dir.join(name), "line\n").expect("造文件");
    }
    std::fs::write(dir.join("keep.txt"), "line\n").expect("造文件");
    let removed = sink.prune().expect("清理");
    assert_eq!(removed, 3, "只删过期的匹配文件");
    assert!(
        !dir.join("app-2026-02-10.log").exists(),
        "过期全量文件需删除"
    );
    assert!(
        !dir.join("app-err-2026-02-10.log").exists(),
        "过期错误文件需删除"
    );
    assert!(
        !dir.join("app-2026-02-12.log").exists(),
        "保留 2 天时 12 号已过期"
    );
    assert!(dir.join("app-2026-02-13.log").exists(), "保留窗口内需保留");
    assert!(
        dir.join("app-err-2026-02-13.log").exists(),
        "保留窗口内需保留"
    );
    assert!(dir.join("app-2026-02-14.log").exists(), "当天文件永不删除");
    assert!(
        dir.join("app-err-2026-02-14.log").exists(),
        "当天错误文件永不删除"
    );
    assert!(
        dir.join("other-2026-02-10.log").exists(),
        "前缀不匹配不得删除"
    );
    assert!(
        dir.join("app-2026-02-10.txt").exists(),
        "扩展名不匹配不得删除"
    );
    assert!(dir.join("app-unmatched.log").exists(), "日期缺失不得删除");
    assert!(dir.join("keep.txt").exists(), "无关文件不得删除");
    assert_eq!(sink.stats().pruned, 3, "清理计数");
}

#[test]
fn write_failure_returns_error_without_panicking() {
    let dir = temp_dir("failure");
    let clock = FakeClock::new(2026, 2, 14);
    let sink = sink(&dir, clock.clone(), Level::Debug);
    std::fs::remove_dir_all(&dir).expect("删掉目录");
    std::fs::write(&dir, "占用路径").expect("用文件占住目录名");
    clock.set(2026, 2, 15);
    let error = sink
        .write(&record(Level::Info, "gateway", "写入"))
        .expect_err("跨天换文件失败时需返回错误");
    assert!(error.to_string().contains("日志文件操作失败"), "{error}");
    assert!(sink.stats().write_errors >= 1, "需累计写失败次数");
    std::fs::remove_file(&dir).expect("清理占位文件");
}

#[test]
fn logger_keeps_memory_records_when_disk_is_unavailable() {
    let dir = temp_dir("memory");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, "占位").expect("造占位文件");
    let file = FileOptions::new(&blocker)
        .with_prefix("app")
        .with_error_prefix("app-err");
    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false)
            .with_file(file),
    );
    assert!(logger.sink().is_none(), "目录不可用时 sink 需降级");
    assert_eq!(logger.sink_errors(), 1, "打开失败需计数");
    logger.info("gateway", "内存里得留着");
    assert_eq!(logger.recent(1).len(), 1, "磁盘不可用不得丢环形缓冲");
    assert_eq!(logger.total(), 1, "总数照常累计");
}

#[test]
fn concurrent_writes_stay_consistent() {
    let dir = temp_dir("concurrency");
    let file = FileOptions::new(&dir)
        .with_prefix("app")
        .with_error_prefix("app-err")
        .with_min_level(Level::Debug);
    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false)
            .with_capacity(4096)
            .with_byte_budget(32 << 20)
            .with_file(file),
    );
    let threads = 8;
    let per_thread = 125;
    let mut handles = Vec::new();
    for index in 0..threads {
        let logger = Arc::clone(&logger);
        handles.push(std::thread::spawn(move || {
            for seq in 0..per_thread {
                logger.info("concurrency", &format!("线程 {index} 第 {seq} 条"));
            }
        }));
    }
    for handle in handles {
        handle.join().expect("线程不得 panic");
    }
    assert_eq!(logger.total(), threads * per_thread, "总条数需精确");
    assert_eq!(
        logger.stats().entries as u64,
        threads * per_thread,
        "环形缓冲需全收"
    );
    logger.flush();
    let sink = logger.sink().expect("sink 存在");
    let (main_path, _) = sink.current_paths();
    let lines = read_lines(&main_path);
    assert_eq!(
        lines.len() as u64,
        threads * per_thread,
        "落盘行数需与写入一致"
    );
    assert_eq!(logger.sink_errors(), 0, "并发落盘不应有失败");
}

#[test]
fn slow_marking_survives_the_logger_path() {
    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false),
    );
    logger.log_timed(
        Level::Info,
        "db",
        "慢查询",
        BTreeMap::new(),
        Duration::from_millis(201),
    );
    logger.log_timed(
        Level::Info,
        "db",
        "边界查询",
        BTreeMap::new(),
        Duration::from_millis(200),
    );
    let records = logger.recent(2);
    assert_eq!(records[0].duration_ms, Some(200), "边界值需原样记录");
    assert!(
        !records[0].is_slow(&SlowThresholds::default()),
        "等于阈值不算慢"
    );
    assert!(
        records[1].is_slow(&SlowThresholds::default()),
        "超过阈值算慢"
    );
    let slow = records[1].render(&logger.format().clone());
    assert!(slow.contains(SLOW_MARK), "渲染需带慢标记: {slow}");
    assert!(!records[0]
        .render(&logger.format().clone())
        .contains(SLOW_MARK));
}

#[test]
fn ring_query_filters_by_level_module_group_user_and_trace() {
    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false),
    );
    logger.debug("imagegen", "生成图片 group=G1 user=U1");
    logger.info("message", "收到消息 group=G1 user=U2");
    logger.warn("db", "慢查询 group=G2 user=U1");
    logger.error("render", "渲染失败");
    let mut receiver = logger.subscribe();
    let _ = logger.recv(&mut receiver);
    let filter = RingFilter::new().with_min_level(Level::Info).with_limit(10);
    let hits = logger.query(&filter);
    assert_eq!(hits.len(), 3, "min_level 需过滤掉 debug");
    assert!(hits.iter().all(|record| record.level != Level::Debug));
    let filter = filter.clone().with_module("message");
    let hits = logger.query(&filter);
    assert_eq!(hits.len(), 1, "模块过滤");
    assert_eq!(hits[0].msg, "收到消息 group=G1 user=U2");
    let filter = RingFilter::new().with_group("G1").with_user("U2");
    let hits = logger.query(&filter);
    assert_eq!(hits.len(), 1, "group+user 组合过滤");
    assert_eq!(hits[0].module, "message");
    let filter = RingFilter::new().with_user("U1");
    let hits = logger.query(&filter);
    assert_eq!(hits.len(), 2, "user 过滤命中两条");
    let mut traced = LogRecord::new(Level::Info, "gateway", "带 trace", BTreeMap::new());
    traced.trace = Some("m3f9a1-17".to_string());
    logger.ring().push(traced);
    let hits = logger.query(&RingFilter::new().with_trace("m3f9a1-17"));
    assert_eq!(hits.len(), 1, "trace 过滤");
    assert_eq!(hits[0].msg, "带 trace");
}

#[test]
fn ring_query_keyword_hits_message_module_and_field_values() {
    let logger = Logger::new(
        Options::default()
            .with_level(Level::Debug)
            .with_console(false),
    );
    logger.info("gateway", "连接建立");
    logger.log(
        Level::Info,
        "imagegen",
        "普通消息",
        BTreeMap::from([("url".to_string(), "https://example/x.png".to_string())]),
    );
    logger.warn("storage", "磁盘告警");
    let hits = logger.query(&RingFilter::new().with_keyword("example"));
    assert_eq!(hits.len(), 1, "keyword 需命中字段值");
    assert_eq!(hits[0].module, "imagegen");
    let hits = logger.query(&RingFilter::new().with_keyword("imagegen"));
    assert_eq!(hits.len(), 1, "keyword 需命中模块名");
    let hits = logger.query(&RingFilter::new().with_keyword("磁盘"));
    assert_eq!(hits.len(), 1, "keyword 需命中正文");
    let hits = logger.query(&RingFilter::new().with_keyword("EXAMPLE"));
    assert_eq!(hits.len(), 1, "keyword 大小写不敏感");
    let hits = logger.query(&RingFilter::new().with_keyword("没有这个"));
    assert!(hits.is_empty(), "未命中需返回空");
}

#[test]
fn ring_query_min_level_is_lower_bound() {
    let ring = Ring::new(16, 1 << 20);
    for level in [Level::Debug, Level::Info, Level::Warn, Level::Error] {
        ring.push(record(level, "gateway", level.as_str()));
    }
    assert_eq!(
        ring.query(&RingFilter::new().with_min_level(Level::Debug))
            .len(),
        4
    );
    assert_eq!(
        ring.query(&RingFilter::new().with_min_level(Level::Info))
            .len(),
        3
    );
    assert_eq!(
        ring.query(&RingFilter::new().with_min_level(Level::Warn))
            .len(),
        2
    );
    assert_eq!(
        ring.query(&RingFilter::new().with_min_level(Level::Error))
            .len(),
        1
    );
}

#[test]
fn ring_query_defaults_limit_to_500_and_returns_newest_first() {
    let ring = Ring::new(1024, 8 << 20);
    for index in 0..600 {
        ring.push(record(Level::Info, "bulk", &format!("第 {index} 条")));
    }
    let hits = ring.query(&RingFilter::new());
    assert_eq!(hits.len(), DEFAULT_QUERY_LIMIT, "limit 为 0 时用默认 500");
    assert_eq!(hits[0].msg, "第 599 条", "最新在前");
    let hits = ring.query(&RingFilter::new().with_limit(3));
    assert_eq!(hits.len(), 3, "显式 limit 生效");
    assert_eq!(hits[2].msg, "第 597 条", "顺序需为倒序");
}

#[test]
fn ring_scopes_are_sorted_and_unique() {
    let ring = Ring::new(16, 1 << 20);
    ring.push(record(Level::Info, "gateway", "a"));
    ring.push(record(Level::Info, "admin", "b"));
    ring.push(record(Level::Info, "gateway", "c"));
    assert_eq!(
        ring.scopes(),
        vec!["admin".to_string(), "gateway".to_string()],
        "模块清单需去重排序"
    );
}

#[test]
fn empty_filter_values_do_not_filter_anything() {
    let ring = Ring::new(16, 1 << 20);
    ring.push(record(Level::Info, "gateway", "a"));
    let filter = RingFilter {
        min_level: None,
        module: Some("  ".to_string()),
        trace: Some(String::new()),
        group: None,
        user: None,
        keyword: Some(String::new()),
        limit: 10,
    };
    assert_eq!(ring.query(&filter).len(), 1, "空白过滤值视作未过滤");
}
