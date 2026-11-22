use std::sync::Arc;
use std::time::Duration;

use aurorix_admin::{JobInfo, JobSource};
use aurorix_api::Outbound;
use aurorix_log::Logger;
use aurorix_plugin::{JobRegistry, JobRun, Outgoing};
use chrono::Utc;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::ports::ReplyTransport;

pub const JOB_TICK: Duration = Duration::from_secs(1);

pub const JOB_TIMEOUT: Duration = Duration::from_secs(60);

pub const JOBS_MODULE: &str = "jobs";

pub struct JobRunner {
    registry: Arc<JobRegistry>,
    transport: Arc<dyn ReplyTransport>,
    logger: Arc<Logger>,
}

impl JobRunner {
    pub fn new(
        registry: Arc<JobRegistry>,
        transport: Arc<dyn ReplyTransport>,
        logger: Arc<Logger>,
    ) -> Self {
        Self {
            registry,
            transport,
            logger,
        }
    }

    pub fn registry(&self) -> &Arc<JobRegistry> {
        &self.registry
    }

    pub fn spawn(self, mut stop: watch::Receiver<bool>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(JOB_TICK);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = ticker.tick() => self.fire_due().await,
                    _ = stop.changed() => {
                        if *stop.borrow() {
                            self.logger
                                .debug(JOBS_MODULE, "定时任务调度器随停机退出");
                            return;
                        }
                    }
                }
            }
        })
    }

    async fn fire_due(&self) {
        let due = self.registry.due(Utc::now());
        for (spec, handler) in due {
            let target = spec.target.clone();
            let run = JobRun {
                job_id: spec.id.clone(),
                plugin_id: spec.plugin_id.clone(),
                target: target.clone(),
                fired_at: Utc::now(),
            };
            let job_id = spec.id.clone();
            let handler = Arc::clone(&handler);
            let mut task = tokio::spawn(async move { handler.run(run).await });
            let joined = match tokio::time::timeout(JOB_TIMEOUT, &mut task).await {
                Ok(joined) => joined,
                Err(_) => {
                    task.abort();
                    self.registry.record_failure(&job_id);
                    self.logger.warn(
                        JOBS_MODULE,
                        &format!(
                            "定时任务 {job_id} 超过 {} 秒未完成, 已中止并继续调度",
                            JOB_TIMEOUT.as_secs()
                        ),
                    );
                    continue;
                }
            };
            let outcome = match joined {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.registry.record_failure(&job_id);
                    self.logger.warn(
                        JOBS_MODULE,
                        &format!("定时任务 {job_id} 的执行任务异常退出: {error}"),
                    );
                    continue;
                }
            };
            match outcome {
                Ok(Some(outgoing)) => {
                    if let Err(error) = self.deliver(&target, &outgoing).await {
                        self.registry.record_failure(&job_id);
                        self.logger.warn(
                            JOBS_MODULE,
                            &format!("定时任务 {job_id} 的回复发送失败: {error}"),
                        );
                        continue;
                    }
                    self.registry.record_fire(&job_id, Utc::now());
                }
                Ok(None) => {
                    self.registry.record_fire(&job_id, Utc::now());
                }
                Err(error) => {
                    self.registry.record_failure(&job_id);
                    self.logger
                        .warn(JOBS_MODULE, &format!("定时任务 {job_id} 执行失败: {error}"));
                }
            }
        }
    }

    async fn deliver(
        &self,
        target: &aurorix_core::Target,
        outgoing: &Outgoing,
    ) -> Result<(), aurorix_api::ApiError> {
        let body = outgoing.body();
        if body.trim().is_empty() && !outgoing.has_image() {
            return Ok(());
        }
        let text = match target.scene.guild() {
            true => aurorix_api::guild_text(&body),
            false => body,
        };
        let message = match target.scene.guild() {
            true => Outbound::text(text),
            false => Outbound::markdown(text),
        }
        .as_initiative();
        self.transport.send(target, &message).await.map(|_| ())
    }
}

pub struct AdminJobs {
    registry: Arc<JobRegistry>,
}

impl AdminJobs {
    pub fn new(registry: Arc<JobRegistry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &Arc<JobRegistry> {
        &self.registry
    }
}

impl JobSource for AdminJobs {
    fn jobs(&self) -> Vec<JobInfo> {
        self.registry
            .reports(Utc::now())
            .into_iter()
            .map(|report| JobInfo {
                id: report.id,
                plugin_id: report.plugin_id,
                kind: report.kind.as_str().to_string(),
                cron: report.cron,
                interval_ms: report.interval_ms,
                immediate: report.immediate,
                paused: report.paused,
                last_fire: report.last_fire,
                next_fire: report.next_fire,
            })
            .collect()
    }

    fn exists(&self, id: &str) -> bool {
        self.registry.exists(id)
    }

    fn set_paused(&self, id: &str, paused: bool) {
        self.registry.set_paused(id, paused);
    }
}
