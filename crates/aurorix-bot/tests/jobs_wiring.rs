use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_admin::JobSource;
use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::jobs::{AdminJobs, JobRunner};
use aurorix_bot::ReplyTransport;
use aurorix_core::{Scene, Target};
use aurorix_gateway::BoxFuture;
use aurorix_log::{Level, Logger, Options};
use aurorix_plugin::{sync_job, JobRegistry, JobRun, JobSpec, Outgoing, ReplyKind};

struct CountingTransport {
    sent: Mutex<Vec<String>>,
}

impl CountingTransport {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sent: Mutex::new(Vec::new()),
        })
    }

    fn contents(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ReplyTransport for CountingTransport {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>> {
        let target = target.clone();
        let content = message.content.clone();
        Box::pin(async move {
            self.sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(content);
            Ok(SendOutcome {
                message_id: Some(String::from("m1")),
                audit_id: None,
                target,
                delivered_active: true,
                fallback: false,
            })
        })
    }
}

fn logger() -> Arc<Logger> {
    Logger::new(Options::default().with_level(Level::Error))
}

fn registry_with(interval: Duration) -> Arc<JobRegistry> {
    let registry = Arc::new(JobRegistry::new());
    registry
        .register(
            JobSpec::interval("heartbeat", "demo", interval)
                .immediate(true)
                .target(Target::new(Scene::Group, "g1")),
            sync_job(|run: &JobRun| {
                Ok(Some(Outgoing::new(
                    ReplyKind::Handler,
                    format!("心跳 {}", run.job_id),
                )))
            }),
        )
        .expect("注册定时任务不应失败");
    registry
}

#[tokio::test]
async fn runner_fires_immediate_jobs_and_delivers() {
    let transport = CountingTransport::new();
    let registry = registry_with(Duration::from_secs(3600));
    let runner = JobRunner::new(
        Arc::clone(&registry),
        Arc::clone(&transport) as Arc<dyn ReplyTransport>,
        logger(),
    );
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let handle = runner.spawn(stop_rx);
    let mut fired = Vec::new();
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        fired = transport.contents();
        if !fired.is_empty() {
            break;
        }
    }
    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert_eq!(
        fired,
        vec![String::from("心跳 heartbeat")],
        "立即触发的任务应当发出一次回复"
    );
    let reports = registry.reports(chrono::Utc::now());
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].runs, 1, "成功执行应当计入 runs");
    assert_eq!(reports[0].failures, 0);
    assert!(reports[0].last_fire > 0, "应当记录上次触发时间");
}

#[tokio::test]
async fn paused_jobs_stop_firing() {
    let transport = CountingTransport::new();
    let registry = registry_with(Duration::from_millis(20));
    registry.set_paused("heartbeat", true);
    let runner = JobRunner::new(
        Arc::clone(&registry),
        Arc::clone(&transport) as Arc<dyn ReplyTransport>,
        logger(),
    );
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let handle = runner.spawn(stop_rx);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(transport.contents().is_empty(), "暂停的任务不应被调度执行");
}

#[tokio::test]
async fn interval_job_repeats() {
    let transport = CountingTransport::new();
    let registry = registry_with(Duration::from_millis(20));
    let runner = JobRunner::new(
        Arc::clone(&registry),
        Arc::clone(&transport) as Arc<dyn ReplyTransport>,
        logger(),
    );
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let handle = runner.spawn(stop_rx);
    let mut count = 0;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        count = transport.contents().len();
        if count >= 2 {
            break;
        }
    }
    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(count >= 2, "间隔任务应当反复触发, 实得 {count} 次");
}

#[test]
fn admin_jobs_reports_live_state() {
    let registry = registry_with(Duration::from_millis(1500));
    let admin = AdminJobs::new(Arc::clone(&registry));
    let listed = admin.jobs();
    assert_eq!(listed.len(), 1, "管理台应当看到真实注册的任务");
    assert_eq!(listed[0].id, "heartbeat");
    assert_eq!(listed[0].plugin_id, "demo");
    assert_eq!(listed[0].kind, "interval");
    assert_eq!(listed[0].interval_ms, 1500);
    assert!(listed[0].immediate, "立即触发标记应当透出");
    assert!(!listed[0].paused);
    assert!(admin.exists("heartbeat"));
    assert!(!admin.exists("nope"), "不存在的任务不应报告存在");

    admin.set_paused("heartbeat", true);
    assert!(admin.jobs()[0].paused, "暂停状态必须反映到管理台");
    admin.set_paused("heartbeat", false);
    assert!(!admin.jobs()[0].paused, "恢复状态必须反映到管理台");
}

#[test]
fn cron_job_is_listed_with_expression() {
    let registry = Arc::new(JobRegistry::new());
    registry
        .register(
            JobSpec::cron("daily", "demo", "0 9 * * *").target(Target::new(Scene::Group, "g1")),
            sync_job(|_run: &JobRun| Ok(None)),
        )
        .expect("cron 任务应当注册成功");
    let admin = AdminJobs::new(registry);
    let listed = admin.jobs();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, "cron");
    assert_eq!(listed[0].cron, "0 9 * * *");
    assert_eq!(listed[0].interval_ms, 0);
    assert!(listed[0].next_fire > 0, "cron 任务应当算出下次触发时间");
}

#[test]
fn handler_returning_none_records_a_run_without_sending() {
    let registry = Arc::new(JobRegistry::new());
    registry
        .register(
            JobSpec::interval("quiet", "demo", Duration::from_secs(60)).immediate(true),
            sync_job(|_run: &JobRun| Ok(None)),
        )
        .expect("注册不应失败");
    let jobs = registry.due(chrono::Utc::now());
    assert_eq!(jobs.len(), 1, "立即触发任务应当在第一次查询时到期");
    let handler = Arc::clone(&jobs[0].1);
    let run = JobRun {
        job_id: String::from("quiet"),
        plugin_id: String::from("demo"),
        target: Target::new(Scene::Group, "g1"),
        fired_at: chrono::Utc::now(),
    };
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("临时运行时")
        .block_on(async move { handler.run(run).await });
    assert!(matches!(outcome, Ok(None)), "静默任务返回 None");
}

#[test]
fn sync_job_exposes_panic_free_contract() {
    let handler = sync_job(|run: &JobRun| {
        Ok(Some(Outgoing::new(
            ReplyKind::Handler,
            format!("{}@{}", run.job_id, run.plugin_id),
        )))
    });
    let run = JobRun {
        job_id: String::from("a"),
        plugin_id: String::from("b"),
        target: Target::new(Scene::C2c, "u1"),
        fired_at: chrono::Utc::now(),
    };
    let produced = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("临时运行时")
        .block_on(async move { handler.run(run).await })
        .expect("不应失败")
        .expect("应当有回复");
    assert_eq!(produced.body(), "a@b");
}
