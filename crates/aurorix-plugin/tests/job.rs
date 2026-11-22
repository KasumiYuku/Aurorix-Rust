use std::time::Duration;

use aurorix_plugin::Scene;
use aurorix_plugin::{
    async_job, sync_job, CronExpr, JobError, JobHandler, JobKind, JobRegistry, JobReport, JobRun,
    JobSchedule, JobSpec, Outgoing, PluginError, ReplyKind, Target, JOB_MODULE,
};
use chrono::{DateTime, Local, TimeZone, Utc};

fn local_at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .expect("固定的本地时间应当存在")
}

fn utc_at(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .expect("固定的时间戳应当有效")
}

fn group_target() -> Target {
    Target::new(Scene::Group, "g1").with_user("u1")
}

fn sample_run(job_id: &str) -> JobRun {
    JobRun {
        job_id: job_id.to_string(),
        plugin_id: "demo".to_string(),
        target: group_target(),
        fired_at: utc_at(1_700_000_000),
    }
}

fn noop_handler() -> std::sync::Arc<dyn JobHandler> {
    sync_job(|_: &JobRun| Ok(None))
}

fn report_of<'a>(reports: &'a [JobReport], id: &str) -> &'a JobReport {
    reports
        .iter()
        .find(|report| report.id == id)
        .expect("报告里应当包含指定任务")
}

#[test]
fn cron_every_five_minutes_lands_on_grid() {
    let expr = CronExpr::parse("*/5 * * * *").expect("每 5 分钟的表达式应当合法");
    assert_eq!(
        expr.next_after(local_at(2024, 3, 1, 10, 2)),
        Some(local_at(2024, 3, 1, 10, 5)),
        "10:02 之后应当落在 10:05"
    );
    assert_eq!(
        expr.next_after(local_at(2024, 3, 1, 10, 5)),
        Some(local_at(2024, 3, 1, 10, 10)),
        "已经站在整点 10:05 时应当严格取下一个 10:10"
    );
    assert_eq!(
        expr.next_after(local_at(2024, 3, 1, 23, 58)),
        Some(local_at(2024, 3, 2, 0, 0)),
        "跨天时应当落在次日 00:00"
    );
}

#[test]
fn cron_weekday_range_hits_workday_morning() {
    let expr = CronExpr::parse("0 9 * * 1-5").expect("工作日表达式应当合法");
    assert_eq!(
        expr.next_after(local_at(2024, 3, 1, 9, 0)),
        Some(local_at(2024, 3, 4, 9, 0)),
        "周五 9:00 之后应当跳到下周一 9:00"
    );
    assert_eq!(
        expr.next_after(local_at(2024, 3, 2, 12, 0)),
        Some(local_at(2024, 3, 4, 9, 0)),
        "周六中午之后应当跳过周日直接到周一 9:00"
    );
    assert_eq!(
        expr.next_after(local_at(2024, 3, 4, 8, 0)),
        Some(local_at(2024, 3, 4, 9, 0)),
        "周一 8:00 之后就是当天 9:00"
    );
}

#[test]
fn cron_first_day_of_month_only() {
    let expr = CronExpr::parse("0 0 1 * *").expect("每月 1 号的表达式应当合法");
    assert_eq!(
        expr.next_after(local_at(2024, 3, 5, 12, 0)),
        Some(local_at(2024, 4, 1, 0, 0)),
        "3 月 5 日之后应当落在 4 月 1 日零点"
    );
    assert_eq!(
        expr.next_after(local_at(2024, 3, 1, 0, 0)),
        Some(local_at(2024, 4, 1, 0, 0)),
        "已经站在 1 号零点时应当严格取下一个月的 1 号"
    );
}

#[test]
fn cron_rejects_invalid_expressions_in_chinese() {
    let short = CronExpr::parse("*/5 * * *").expect_err("4 个字段应当被拒绝");
    assert!(
        short.contains("5 个字段") && short.contains("4 个"),
        "字段数量错误的中文提示应当写明 5 与 4，实际是: {short}"
    );
    let minute = CronExpr::parse("61 0 * * *").expect_err("61 分钟应当被拒绝");
    assert!(
        minute.contains("分钟") && minute.contains("61") && minute.contains("0-59"),
        "分钟越界的中文提示应当写明字段与范围，实际是: {minute}"
    );
    let weekday = CronExpr::parse("* * * * 8").expect_err("星期 8 应当被拒绝");
    assert!(
        weekday.contains("星期") && weekday.contains("8") && weekday.contains("0-7"),
        "星期越界的中文提示应当写明字段与范围，实际是: {weekday}"
    );
    assert_eq!(CronExpr::FIELD_COUNT, 5, "cron 表达式固定为 5 个字段");
}

#[test]
fn cron_day_and_weekday_use_or_semantics() {
    let expr = CronExpr::parse("0 0 13 * 5").expect("13 号或周五的表达式应当合法");
    let first = expr
        .next_after(local_at(2024, 4, 1, 0, 0))
        .expect("应当有下一次触发");
    assert_eq!(
        first,
        local_at(2024, 4, 5, 0, 0),
        "4 月 5 日是周五，应当先于 13 号命中"
    );
    let second = expr.next_after(first).expect("应当有下一次触发");
    assert_eq!(
        second,
        local_at(2024, 4, 12, 0, 0),
        "下一个周五 4 月 12 日应当命中"
    );
    let third = expr.next_after(second).expect("应当有下一次触发");
    assert_eq!(
        third,
        local_at(2024, 4, 13, 0, 0),
        "4 月 13 日是周六，但它是 13 号，按“或”语义应当命中"
    );
    let fourth = expr.next_after(third).expect("应当有下一次触发");
    assert_eq!(
        fourth,
        local_at(2024, 4, 19, 0, 0),
        "13 号之后应当回到周五 4 月 19 日"
    );
}

#[test]
fn cron_accepts_question_mark_and_sunday_seven() {
    let expr = CronExpr::parse("0 0 ? * 7").expect("问号与数字 7 都应当被接受");
    assert_eq!(
        expr.next_after(local_at(2024, 3, 2, 0, 0)),
        Some(local_at(2024, 3, 3, 0, 0)),
        "星期 7 等价星期 0，应当命中周日"
    );
    let zero = CronExpr::parse("0 0 * * 0").expect("星期 0 应当合法");
    assert_eq!(
        zero.next_after(local_at(2024, 3, 3, 0, 0)),
        Some(local_at(2024, 3, 10, 0, 0)),
        "周日零点之后应当严格取下一个周日"
    );
}

#[test]
fn cron_returns_none_when_no_match_within_five_years() {
    let expr = CronExpr::parse("0 0 30 2 *").expect("语法本身合法");
    assert_eq!(
        expr.next_after(local_at(2024, 1, 1, 0, 0)),
        None,
        "2 月没有 30 号，搜索 5 年也应当返回 None"
    );
}

#[test]
fn job_error_messages_are_chinese() {
    assert_eq!(JobError::MissingId.to_string(), "任务 id 不能为空");
    assert_eq!(JobError::MissingHandler.to_string(), "任务处理器不能为空");
    assert!(
        JobError::MissingSchedule.to_string().contains("调度配置"),
        "缺少调度配置的提示应当是中文"
    );
    assert_eq!(
        JobError::InvalidCron("字段数量不对".to_string()).to_string(),
        "cron 表达式不合法：字段数量不对"
    );
    assert_eq!(
        JobError::Duplicate("a".to_string()).to_string(),
        "任务 a 已经存在"
    );
    assert_eq!(
        JobError::Unknown("a".to_string()).to_string(),
        "任务 a 不存在"
    );
}

#[test]
fn register_rejects_missing_id() {
    let registry = JobRegistry::new();
    let err = registry
        .register(
            JobSpec::interval("   ", "demo", Duration::from_secs(60)),
            noop_handler(),
        )
        .expect_err("空 id 应当被拒绝");
    assert_eq!(err, JobError::MissingId, "空 id 应当返回 MissingId");
    assert_eq!(registry.len(), 0, "被拒绝的任务不应进入注册表");
}

#[test]
fn register_rejects_missing_or_invalid_schedule() {
    let registry = JobRegistry::new();
    let bad_cron = registry
        .register(JobSpec::cron("tick", "demo", "0 0 * *"), noop_handler())
        .expect_err("不合格的 cron 应当被拒绝");
    assert!(
        matches!(&bad_cron, JobError::InvalidCron(detail) if detail.contains("5 个字段")),
        "不合格 cron 应当返回 InvalidCron，实际是: {bad_cron}"
    );
    let empty_cron = registry
        .register(JobSpec::cron("tick", "demo", "   "), noop_handler())
        .expect_err("空 cron 应当被拒绝");
    assert_eq!(
        empty_cron,
        JobError::MissingSchedule,
        "空 cron 视为缺少调度"
    );
    let zero_span = registry
        .register(
            JobSpec::interval("tick", "demo", Duration::from_secs(0)),
            noop_handler(),
        )
        .expect_err("零间隔应当被拒绝");
    assert_eq!(zero_span, JobError::MissingSchedule, "零间隔视为缺少调度");
    assert_eq!(registry.len(), 0, "被拒绝的任务不应进入注册表");
}

#[tokio::test]
async fn register_duplicate_id_overwrites_previous_job() {
    let registry = JobRegistry::new();
    registry
        .register(
            JobSpec::interval("tick", "old", Duration::from_secs(60)).immediate(true),
            sync_job(|_: &JobRun| Ok(Some(Outgoing::new(ReplyKind::Handler, "旧处理器")))),
        )
        .expect("第一次注册应当成功");
    registry
        .register(
            JobSpec::interval("tick", "new", Duration::from_secs(60)).immediate(true),
            sync_job(|_: &JobRun| Ok(Some(Outgoing::new(ReplyKind::Handler, "新处理器")))),
        )
        .expect("同 id 注册应当覆盖旧任务");
    assert_eq!(registry.len(), 1, "同 id 只应保留一条任务");
    let reports = registry.reports(Utc::now());
    assert_eq!(reports.len(), 1, "报告里也只应出现一条任务");
    assert_eq!(reports[0].plugin_id, "new", "覆盖后应当是新插件");
    let due = registry.due(Utc::now());
    assert_eq!(due.len(), 1, "覆盖后的任务仍然立刻到期");
    let answer = due[0]
        .1
        .run(sample_run("tick"))
        .await
        .expect("处理器应当成功返回");
    assert_eq!(
        answer.map(|message| message.body()),
        Some("新处理器".to_string()),
        "到期返回的应当是覆盖后的新处理器"
    );
}

#[test]
fn pause_resume_cancel_and_len() {
    let registry = JobRegistry::new();
    assert!(registry.is_empty(), "新建注册表应当是空的");
    assert_eq!(registry.len(), 0, "新建注册表长度为 0");
    registry
        .register(JobSpec::cron("tick", "demo", "0 0 * * *"), noop_handler())
        .expect("注册应当成功");
    assert_eq!(registry.len(), 1, "注册后长度为 1");
    assert!(registry.exists("tick"), "注册后任务应当存在");
    assert!(!registry.exists("missing"), "未注册的任务不应存在");
    assert!(registry.pause("tick"), "暂停已存在的任务应当返回 true");
    assert!(
        registry.reports(Utc::now())[0].paused,
        "暂停后报告应当标记 paused"
    );
    assert!(registry.resume("tick"), "恢复已暂停的任务应当返回 true");
    assert!(
        !registry.reports(Utc::now())[0].paused,
        "恢复后应当取消 paused"
    );
    assert!(!registry.pause("missing"), "暂停不存在的任务应当返回 false");
    assert!(
        !registry.resume("missing"),
        "恢复不存在的任务应当返回 false"
    );
    assert!(registry.cancel("tick"), "取消已存在的任务应当返回 true");
    assert!(!registry.cancel("tick"), "重复取消应当返回 false");
    assert!(registry.is_empty(), "取消之后注册表应当为空");
}

#[test]
fn due_fires_immediate_interval_once() {
    let registry = JobRegistry::new();
    registry
        .register(
            JobSpec::interval("tick", "demo", Duration::from_secs(1800)).immediate(true),
            noop_handler(),
        )
        .expect("注册应当成功");
    let now = Utc::now();
    let first = registry.due(now);
    assert_eq!(first.len(), 1, "immediate 任务第一次查询就应当到期");
    assert_eq!(first[0].0.id, "tick", "到期的应当是 tick 任务");
    assert_eq!(first[0].0.plugin_id, "demo", "到期的任务应当带插件 id");
    assert_eq!(
        first[0].0.schedule,
        JobSchedule::Interval(Duration::from_secs(1800)),
        "到期任务的调度应当与注册时一致"
    );
    let again = registry.due(now);
    assert!(again.is_empty(), "同一个 now 第二次查询不应重复到期");
}

#[test]
fn due_waits_full_interval_when_not_immediate() {
    let registry = JobRegistry::new();
    registry
        .register(
            JobSpec::interval("tick", "demo", Duration::from_secs(3600)),
            noop_handler(),
        )
        .expect("注册应当成功");
    assert!(
        registry.due(Utc::now()).is_empty(),
        "非 immediate 任务刚注册时不应到期"
    );
    let later = Utc::now() + chrono::Duration::seconds(3601);
    assert_eq!(registry.due(later).len(), 1, "满一个间隔后应当到期一次");
    assert!(
        registry.due(later).is_empty(),
        "到期之后同一个 now 不应重复返回"
    );
}

#[test]
fn due_skips_paused_jobs() {
    let registry = JobRegistry::new();
    registry
        .register(
            JobSpec::interval("tick", "demo", Duration::from_secs(60)).immediate(true),
            noop_handler(),
        )
        .expect("注册应当成功");
    registry.pause("tick");
    assert!(registry.due(Utc::now()).is_empty(), "暂停的任务不应到期");
    registry.resume("tick");
    assert_eq!(registry.due(Utc::now()).len(), 1, "恢复后的任务应当到期");
}

#[test]
fn record_fire_and_failure_update_reports() {
    let registry = JobRegistry::new();
    registry
        .register(
            JobSpec::interval("tick", "demo", Duration::from_secs(60)).immediate(true),
            noop_handler(),
        )
        .expect("注册应当成功");
    let before = registry.reports(Utc::now());
    assert_eq!(
        report_of(&before, "tick").last_fire,
        0,
        "从未触发时 last_fire 应当是 0"
    );
    assert_eq!(
        report_of(&before, "tick").runs,
        0,
        "从未触发时 runs 应当是 0"
    );
    let fired = utc_at(1_700_000_000);
    registry.record_fire("tick", fired);
    registry.record_fire("tick", fired);
    let after = registry.reports(Utc::now());
    assert_eq!(
        report_of(&after, "tick").last_fire,
        fired.timestamp_millis(),
        "记录触发后 last_fire 应当等于触发时刻的毫秒时间戳"
    );
    assert_eq!(
        report_of(&after, "tick").runs,
        2,
        "触发两次后 runs 应当是 2"
    );
    registry.record_failure("tick");
    registry.record_failure("missing");
    let failed = registry.reports(Utc::now());
    assert_eq!(
        report_of(&failed, "tick").failures,
        1,
        "记录一次失败后 failures 应当是 1"
    );
    assert_eq!(report_of(&failed, "tick").runs, 2, "记录失败不应影响 runs");
}

#[test]
fn reports_describe_interval_and_cron_specs() {
    let registry = JobRegistry::new();
    registry
        .register(
            JobSpec::interval("every", "demo", Duration::from_millis(1500))
                .immediate(true)
                .target(group_target()),
            noop_handler(),
        )
        .expect("注册间隔任务应当成功");
    registry
        .register(
            JobSpec::cron("daily", "demo", "0 9 * * 1-5"),
            noop_handler(),
        )
        .expect("注册 cron 任务应当成功");
    let now = Utc::now();
    let reports = registry.reports(now);
    assert_eq!(reports.len(), 2, "两个任务都应当出现在报告里");
    let every = report_of(&reports, "every");
    assert_eq!(
        every.kind,
        JobKind::Interval,
        "间隔任务的 kind 应当是 interval"
    );
    assert_eq!(
        every.kind.as_str(),
        "interval",
        "kind 的字符串形式应当是 interval"
    );
    assert_eq!(every.cron, "", "间隔任务没有 cron 文本");
    assert_eq!(every.interval_ms, 1500, "间隔任务应当报告 1500 毫秒");
    assert!(every.immediate, "间隔任务应当记录 immediate");
    assert!(!every.paused, "新注册任务不应是暂停状态");
    assert!(every.next_fire > 0, "间隔任务的 next_fire 不应当是 0");
    let daily = report_of(&reports, "daily");
    assert_eq!(daily.kind, JobKind::Cron, "cron 任务的 kind 应当是 cron");
    assert_eq!(daily.kind.as_str(), "cron", "kind 的字符串形式应当是 cron");
    assert_eq!(daily.cron, "0 9 * * 1-5", "cron 文本应当原样报告");
    assert_eq!(daily.interval_ms, 0, "cron 任务没有间隔毫秒");
    assert!(daily.next_fire > 0, "cron 任务的 next_fire 不应当是 0");
    assert_eq!(
        registry.reports(now),
        reports,
        "reports 只是只读快照，连续读取应当一致"
    );
    assert_eq!(JOB_MODULE, "job", "任务子系统的模块名固定为 job");
}

#[tokio::test]
async fn async_and_sync_jobs_both_answer() {
    let async_handler = async_job(|run: JobRun| async move {
        Ok(Some(Outgoing::new(
            ReplyKind::Handler,
            format!(
                "异步任务 {} 于 {} 触发",
                run.job_id,
                run.fired_at.timestamp()
            ),
        )))
    });
    let answer = async_handler
        .run(sample_run("async"))
        .await
        .expect("异步处理器应当成功");
    assert_eq!(
        answer.map(|message| message.body()),
        Some("异步任务 async 于 1700000000 触发".to_string()),
        "异步处理器应当拿到 JobRun 并返回消息"
    );
    let quiet_async = async_job(|_: JobRun| async move { Ok(None) });
    assert!(
        quiet_async
            .run(sample_run("async"))
            .await
            .expect("异步处理器应当成功")
            .is_none(),
        "异步处理器可以返回 None"
    );

    let sync_handler = sync_job(|run: &JobRun| {
        if run.job_id == "sync" {
            Ok(Some(Outgoing::new(ReplyKind::Handler, "同步任务")))
        } else {
            Err(PluginError::handler("不认识的任务"))
        }
    });
    let answer = sync_handler
        .run(sample_run("sync"))
        .await
        .expect("同步处理器应当成功");
    assert_eq!(
        answer.map(|message| message.body()),
        Some("同步任务".to_string()),
        "同步处理器应当返回消息"
    );
    let quiet_sync = sync_job(|_: &JobRun| Ok(None));
    assert!(
        quiet_sync
            .run(sample_run("sync"))
            .await
            .expect("同步处理器应当成功")
            .is_none(),
        "同步处理器可以返回 None"
    );
    let failing = sync_handler
        .run(sample_run("other"))
        .await
        .expect_err("未知任务应当报错");
    assert_eq!(
        failing.to_string(),
        "指令执行失败: 不认识的任务",
        "处理器错误应当保持中文文案"
    );
}

#[test]
fn registry_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<JobRegistry>();
    let registry = std::sync::Arc::new(JobRegistry::new());
    let shared = std::sync::Arc::clone(&registry);
    let handle = std::thread::spawn(move || {
        shared
            .register(JobSpec::cron("tick", "demo", "0 0 * * *"), noop_handler())
            .expect("注册应当成功");
    });
    handle.join().expect("线程应当正常结束");
    assert_eq!(registry.len(), 1, "跨线程注册后应当能看到任务");
    assert!(
        matches!(registry.due(Utc::now()), due if due.is_empty()),
        "cron 任务此刻不应到期"
    );
}
