use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aurorix_admin::{
    AdminState, Control, ControlAction, Counts as AdminCounts, GatewayStatus, LogSource,
    PluginError, PluginInfo, PluginSource, StatsSource, StatusSource, StatusView,
};
use aurorix_api::BotApi;
use aurorix_core::{RoleRequired, Scene, Target};
use aurorix_db::AppConfig;
use aurorix_gateway::{
    EventSink, GatewayClient, GatewayState, ProtocolEvent, TokenSource, Transport,
    WebSocketTransport,
};
use aurorix_log::{Level, Logger, Options};
use aurorix_plugin::{
    AccessConfig as PluginAccessConfig, AccessMode, AccessRule as PluginAccessRule,
    CommandRegistry, Deny as PluginDeny, Maintenance as PluginMaintenance, Outgoing, RoleResolver,
    Router,
};
use aurorix_template::Registry as TemplateRegistry;
use axum::Router as AxumRouter;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::{BotConfig, DEFAULT_PROTOCOL};
use crate::error::BotError;
use crate::event::EventCategory;
use crate::gateway_source::OfficialGatewaySource;
use crate::lock::InstanceLock;
use crate::pipeline::{Pipeline, PipelineCounters, PipelineDeps, PushReport};
use crate::plugin_config::PluginConfigStore;
use crate::ports::{
    ApiTokens, InboxArchive, KvBackend, MemoryKv, MemoryStats, PluginConfigurable, Renderer,
    ReplyTransport, StatsRecorder,
};
use crate::webhook::{ensure_webhook_intents, WebhookIngress, WEBHOOK_MODE_LABEL};

pub const MODULE: &str = "bot";

pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
pub const ADMIN_DRAIN: Duration = Duration::from_millis(500);

pub const MEMORY_HEARTBEAT: Duration = Duration::from_secs(60);

pub const DEFAULT_EVENT_BUFFER: usize = 1024;

pub const CONTROL_BUFFER: usize = 8;

pub const SUPERVISED_ENV: &str = "AURORIX_SUPERVISED";

pub const UNSUPERVISED_RESTART_ERROR: &str = aurorix_admin::UNSUPERVISED_RESTART_ERROR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackPolicy {
    Deny,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCommand {
    Stop,
    Restart,
}

impl ControlCommand {
    pub const fn label(self) -> &'static str {
        match self {
            ControlCommand::Stop => "停止",
            ControlCommand::Restart => "重启",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    pub control: ControlCommand,
    pub live_tasks: usize,
    pub gateway_state: GatewayState,
    pub timed_out: bool,
    pub elapsed_ms: u64,
}

struct TaskGuard {
    live: Arc<AtomicUsize>,
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ChannelSink {
    tx: mpsc::Sender<Arc<ProtocolEvent>>,
    logger: Arc<Logger>,
    dropped: AtomicU64,
}

impl EventSink for ChannelSink {
    fn dispatch(&self, event: ProtocolEvent) {
        if self.tx.try_send(Arc::new(event)).is_err() {
            let count = self.dropped.fetch_add(1, Ordering::SeqCst) + 1;
            self.logger.warn(
                MODULE,
                &format!("事件队列已满或已关闭, 丢弃事件(累计 {count} 条)"),
            );
        }
    }
}

pub struct Runtime {
    config: Arc<BotConfig>,
    app_config: Arc<Mutex<AppConfig>>,
    config_path: PathBuf,
    logger: Arc<Logger>,
    pipeline: Arc<Pipeline>,
    gateway: Arc<GatewayClient>,
    control_tx: mpsc::Sender<ControlCommand>,
    control_rx: Mutex<Option<mpsc::Receiver<ControlCommand>>>,
    events_rx: Mutex<Option<mpsc::Receiver<Arc<ProtocolEvent>>>>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    live: Arc<AtomicUsize>,
    gateway_slot: AtomicBool,
    outcome: Mutex<Option<ControlCommand>>,
    done_tx: watch::Sender<bool>,
    done_rx: watch::Receiver<bool>,
    started_at: Instant,
    config_failures: Mutex<Vec<String>>,
    plugin_config: Arc<PluginConfigStore>,
    panel_remote: Option<Arc<dyn aurorix_admin::PanelRemote>>,
    sink: Arc<dyn EventSink>,
    jobs: Arc<aurorix_plugin::JobRegistry>,
    database: Option<Arc<aurorix_db::Db>>,
    assets_reload: Option<Arc<dyn aurorix_admin::AssetReload>>,
    instance_lock: Mutex<Option<InstanceLock>>,
    report: Mutex<Option<ShutdownReport>>,
    template_reload: Option<aurorix_template::ReloadHandle>,
    events_bus: Arc<aurorix_admin::LiveBus>,
}

fn lock<T>(target: &Mutex<T>) -> MutexGuard<'_, T> {
    target
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Runtime {
    pub fn config(&self) -> &Arc<BotConfig> {
        &self.config
    }

    pub fn app_config(&self) -> AppConfig {
        lock(&self.app_config).clone()
    }

    pub fn plugin_config(&self) -> &Arc<PluginConfigStore> {
        &self.plugin_config
    }

    pub fn template_reload_count(&self) -> u64 {
        self.template_reload
            .as_ref()
            .map(|handle| handle.overrides().reload_count())
            .unwrap_or(0)
    }

    pub fn template_reload_active(&self) -> bool {
        self.template_reload
            .as_ref()
            .is_some_and(|handle| handle.is_running())
    }

    pub fn logger(&self) -> &Arc<Logger> {
        &self.logger
    }

    pub fn pipeline(&self) -> &Arc<Pipeline> {
        &self.pipeline
    }

    pub fn gateway(&self) -> &Arc<GatewayClient> {
        &self.gateway
    }

    pub fn gateway_state(&self) -> GatewayState {
        self.gateway.state()
    }

    pub fn counters(&self) -> PipelineCounters {
        self.pipeline.counters()
    }

    pub fn live_tasks(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }

    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    pub fn config_failures(&self) -> Vec<String> {
        lock(&self.config_failures).clone()
    }

    pub fn outcome(&self) -> ControlCommand {
        lock(&self.outcome).unwrap_or(ControlCommand::Stop)
    }

    pub fn is_finished(&self) -> bool {
        *self.done_rx.borrow()
    }

    pub fn is_webhook(&self) -> bool {
        self.config.protocol != DEFAULT_PROTOCOL
    }

    pub fn release_instance_lock(&self) {
        lock(&self.instance_lock).take();
    }

    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.live.fetch_add(1, Ordering::SeqCst);
        let live = Arc::clone(&self.live);
        let handle = tokio::spawn(async move {
            let _guard = TaskGuard { live };
            future.await;
        });
        lock(&self.tasks).push(handle);
    }

    fn take_tasks(&self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut *lock(&self.tasks))
    }

    fn release_gateway_slot(&self) {
        if self.gateway_slot.swap(false, Ordering::SeqCst) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn take_report(&self) -> Option<ShutdownReport> {
        lock(&self.report).clone()
    }

    fn set_report(&self, report: ShutdownReport) {
        *lock(&self.report) = Some(report);
    }

    fn set_outcome(&self, command: ControlCommand) {
        *lock(&self.outcome) = Some(command);
    }

    fn set_config_failures(&self, failures: Vec<String>) {
        *lock(&self.config_failures) = failures;
    }
}

#[derive(Clone)]
pub struct RuntimeHandle {
    runtime: Arc<Runtime>,
}

impl std::fmt::Debug for RuntimeHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeHandle")
            .field("live_tasks", &self.live_tasks())
            .field("gateway", &self.gateway_state())
            .finish()
    }
}

impl RuntimeHandle {
    pub fn new(runtime: Arc<Runtime>) -> Self {
        Self { runtime }
    }

    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }

    pub fn pipeline(&self) -> &Arc<Pipeline> {
        self.runtime.pipeline()
    }

    pub fn logger(&self) -> &Arc<Logger> {
        self.runtime.logger()
    }

    pub fn live_tasks(&self) -> usize {
        self.runtime.live_tasks()
    }

    pub fn gateway_state(&self) -> GatewayState {
        self.runtime.gateway_state()
    }

    pub fn counters(&self) -> PipelineCounters {
        self.runtime.counters()
    }

    pub fn config_failures(&self) -> Vec<String> {
        self.runtime.config_failures()
    }

    pub fn is_finished(&self) -> bool {
        self.runtime.is_finished()
    }

    pub fn send(&self, command: ControlCommand) -> bool {
        self.runtime.control_tx.try_send(command).is_ok()
    }

    pub fn request_stop(&self) -> bool {
        self.send(ControlCommand::Stop)
    }

    pub fn request_restart(&self) -> bool {
        self.send(ControlCommand::Restart)
    }

    pub async fn wait(&self) -> ControlCommand {
        let mut done = self.runtime.done_rx.clone();
        loop {
            if *done.borrow() {
                break;
            }
            if done.changed().await.is_err() {
                break;
            }
        }
        self.runtime.outcome()
    }
}

pub struct Bot {
    runtime: Arc<Runtime>,
    config_sinks: Vec<Arc<dyn PluginConfigurable>>,
    signal_handling: bool,
    started: AtomicBool,
    instance_lock: bool,
}

impl std::fmt::Debug for Bot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Bot")
            .field("appid", &self.runtime.config.appid)
            .field("started", &self.started.load(Ordering::SeqCst))
            .field("live_tasks", &self.runtime.live_tasks())
            .finish()
    }
}

impl Bot {
    pub fn builder(config: BotConfig) -> BotBuilder {
        BotBuilder::new(config)
    }

    pub fn config(&self) -> &Arc<BotConfig> {
        self.runtime.config()
    }

    pub fn logger(&self) -> &Arc<Logger> {
        self.runtime.logger()
    }

    pub fn pipeline(&self) -> &Arc<Pipeline> {
        self.runtime.pipeline()
    }

    pub async fn push(&self, target: &Target, scene: Scene, reply: &Outgoing) -> PushReport {
        self.runtime.pipeline().push(target, scene, reply).await
    }

    pub fn handle(&self) -> RuntimeHandle {
        RuntimeHandle::new(Arc::clone(&self.runtime))
    }

    pub fn is_webhook(&self) -> bool {
        self.runtime.is_webhook()
    }

    pub fn live_tasks(&self) -> usize {
        self.runtime.live_tasks()
    }

    pub fn gateway_state(&self) -> GatewayState {
        self.runtime.gateway_state()
    }

    pub fn config_failures(&self) -> Vec<String> {
        self.runtime.config_failures()
    }

    pub fn counters(&self) -> PipelineCounters {
        self.runtime.counters()
    }

    pub async fn start(&self) -> Result<RuntimeHandle, BotError> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Err(BotError::AlreadyStarted(String::from(
                "同一个 Bot 只允许启动一次, 重启请重新构建",
            )));
        }
        let runtime = Arc::clone(&self.runtime);
        if runtime.is_webhook() {
            ensure_webhook_intents(&runtime.config.intents)?;
        }
        self.acquire_instance_lock()?;
        self.log_start_step(
            "1/7 初始化数据层",
            vec![
                ("kv", yes_no(runtime.pipeline.has_kv())),
                ("inbox", yes_no(runtime.pipeline.has_inbox())),
                ("stats", yes_no(runtime.pipeline.has_stats())),
                ("database", runtime.config.database.clone()),
            ],
        );
        let registry = Arc::clone(runtime.pipeline.router().registry());
        let plugins = registry.plugins();
        for slot in &plugins {
            if slot.commands.is_empty() {
                runtime.logger.warn(
                    MODULE,
                    &format!("插件 {} 没有注册任何指令, 已忽略", slot.id),
                );
            }
        }
        self.log_start_step(
            "2/7 加载插件",
            vec![
                ("plugins", plugins.len().to_string()),
                ("commands", registry.command_count().to_string()),
            ],
        );
        let failures = self.apply_plugin_configs();
        let applied = self.config_sinks.len().saturating_sub(failures.len());
        if !failures.is_empty() {
            runtime.logger.warn(
                MODULE,
                &format!(
                    "部分插件配置未生效({} 个), 其它插件照常运行: {}",
                    failures.len(),
                    failures.join("; ")
                ),
            );
        }
        runtime.set_config_failures(failures.clone());
        self.log_start_step(
            "3/7 加载插件配置",
            vec![
                ("applied", applied.to_string()),
                ("failed", failures.len().to_string()),
            ],
        );
        self.apply_live_config();
        self.load_bans().await;
        self.log_start_step(
            "4/7 注册指令",
            vec![
                ("plugins", plugins.len().to_string()),
                ("commands", registry.command_count().to_string()),
                ("aliases", registry.index().alias_count().to_string()),
                ("prefixes", format!("{:?}", runtime.config.prefixes)),
            ],
        );
        if runtime.is_webhook() {
            self.log_start_step(
                "5/7 跳过 WebSocket 网关",
                vec![
                    ("protocol", runtime.config.protocol.clone()),
                    ("path", String::from(WebhookIngress::path())),
                ],
            );
        } else {
            runtime.live.fetch_add(1, Ordering::SeqCst);
            runtime.gateway_slot.store(true, Ordering::SeqCst);
            runtime.gateway.start();
            self.log_start_step(
                "5/7 启动 WebSocket 网关",
                vec![
                    ("url", runtime.config.gateway_config().url),
                    ("intents", runtime.config.intent_bits().to_string()),
                ],
            );
        }
        self.start_admin().await?;
        self.spawn_jobs();
        self.spawn_memory_heartbeat();
        self.spawn_event_loop();
        self.log_start_step(
            "7/7 进入事件循环",
            vec![("controls", String::from("网关事件/关闭信号/管理台控制"))],
        );
        Ok(RuntimeHandle::new(runtime))
    }

    pub async fn serve(&self) -> Result<ShutdownReport, BotError> {
        let handle = self.start().await?;
        let control = handle.wait().await;
        self.runtime.logger.info(
            MODULE,
            &format!("收到{}指令, 开始优雅停机", control.label()),
        );
        self.shutdown(DEFAULT_SHUTDOWN_TIMEOUT).await
    }

    pub async fn shutdown(&self, timeout: Duration) -> Result<ShutdownReport, BotError> {
        if let Some(report) = self.runtime.take_report() {
            return Ok(report);
        }
        let started = Instant::now();
        let runtime = Arc::clone(&self.runtime);
        let gateway = Arc::clone(&runtime.gateway);
        let gateway_timeout = timeout.max(Duration::from_millis(1));
        match tokio::time::timeout(gateway_timeout, async move { gateway.shutdown().await }).await {
            Ok(()) => runtime.logger.info(MODULE, "网关已停止, 不再接收新事件"),
            Err(_) => runtime
                .logger
                .warn(MODULE, "网关停机超时, 继续排空在飞任务"),
        }
        runtime.release_gateway_slot();
        let _ = runtime.stop_tx.send(true);
        let tasks = runtime.take_tasks();
        let mut timed_out = false;
        for handle in tasks {
            let budget = timeout.saturating_sub(started.elapsed());
            let mut handle = handle;
            if budget.is_zero() {
                handle.abort();
                timed_out = true;
                continue;
            }
            if tokio::time::timeout(budget, &mut handle).await.is_err() {
                handle.abort();
                timed_out = true;
            }
        }
        for _ in 0..4 {
            tokio::task::yield_now().await;
        }
        runtime.pipeline.close().await;
        runtime.release_instance_lock();
        let live_tasks = runtime.live_tasks();
        if timed_out {
            runtime.logger.warn(
                MODULE,
                &format!("停机超时, 已强制中止后台任务, 残留 {live_tasks} 个"),
            );
        }
        let report = ShutdownReport {
            control: runtime.outcome(),
            live_tasks,
            gateway_state: runtime.gateway.state(),
            timed_out,
            elapsed_ms: started.elapsed().as_millis() as u64,
        };
        runtime.logger.info(
            MODULE,
            &format!(
                "停机完成: 指令={}, 网关={}, 残留任务={}, 耗时={}ms",
                report.control.label(),
                report.gateway_state.as_str(),
                report.live_tasks,
                report.elapsed_ms
            ),
        );
        runtime.set_report(report.clone());
        Ok(report)
    }

    fn acquire_instance_lock(&self) -> Result<(), BotError> {
        if !self.instance_lock {
            return Ok(());
        }
        let runtime = &self.runtime;
        let path = runtime.config.lock_file_path();
        let instance = InstanceLock::acquire(&path)?;
        *lock(&runtime.instance_lock) = Some(instance);
        runtime
            .logger
            .info(MODULE, &format!("单实例锁已获取: {path}"));
        Ok(())
    }

    fn apply_plugin_configs(&self) -> Vec<String> {
        let mut failures = Vec::new();
        for sink in &self.config_sinks {
            let id = sink.plugin_id().to_string();
            let settings = self
                .runtime
                .config
                .plugin_settings
                .get(&id)
                .cloned()
                .unwrap_or_default();
            if let Err(error) = sink.apply(&settings) {
                failures.push(format!("插件 {id} 的配置未生效, 退回默认值: {error}"));
            }
        }
        failures
    }

    fn apply_live_config(&self) {
        let runtime = &self.runtime;
        let config = runtime.app_config();
        let router = runtime.pipeline.router();
        apply_access(router, &config);
        apply_maintenance(&runtime.pipeline, router, &config);
    }

    async fn load_bans(&self) {
        let runtime = &self.runtime;
        let Some(database) = &runtime.database else {
            runtime
                .logger
                .debug(MODULE, "未注入数据库, 封禁名单保持为空");
            return;
        };
        match database.bans().load().await {
            Ok(entries) => {
                let count = entries.len();
                runtime
                    .pipeline
                    .router()
                    .ban()
                    .load(entries.into_iter().map(to_plugin_ban));
                runtime
                    .logger
                    .info(MODULE, &format!("封禁名单已加载: {count} 条"));
            }
            Err(error) => runtime.logger.warn(
                MODULE,
                &format!("读取封禁名单失败, 名单保持为空: {error}"),
            ),
        }
    }

    async fn start_admin(&self) -> Result<(), BotError> {
        let runtime = Arc::clone(&self.runtime);
        let webhook = runtime.is_webhook();
        let admin = !runtime.config.admin_password.trim().is_empty();
        if !admin && !webhook {
            self.log_start_step(
                "6/7 启动管理台 HTTP",
                vec![("state", String::from("未启用(admin_password 为空)"))],
            );
            return Ok(());
        }
        let mut app = AxumRouter::new();
        let mut entry = String::from("未启用(admin_password 为空)");
        let mut fields: Vec<(&str, String)> = Vec::new();
        if admin {
            app = app.merge(self.admin_router());
            entry = String::from("已启用");
        }
        fields.push(("admin", entry));
        if webhook {
            let ingress =
                WebhookIngress::new(runtime.config.secret.clone(), Arc::clone(&runtime.sink));
            app = app.merge(ingress.into_router());
            fields.push(("webhook", String::from(WebhookIngress::path())));
        }
        let addr = runtime.config.admin_addr();
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|err| BotError::Admin(format!("管理台无法监听 {addr}: {err}")))?;
        if webhook {
            runtime.logger.info(
                MODULE,
                &format!(
                    "{WEBHOOK_MODE_LABEL}: POST http://{addr}{}, 收到的事件进入统一事件循环",
                    WebhookIngress::path()
                ),
            );
        }
        let mut stop = runtime.stop_rx.clone();
        let logger = Arc::clone(&runtime.logger);
        runtime.spawn(async move {
            let (closing_tx, closing_rx) = tokio::sync::oneshot::channel();
            let shutdown = async move {
                let _ = stop.changed().await;
                let _ = closing_tx.send(());
            };
            let serve = axum::serve(listener, app).with_graceful_shutdown(shutdown);
            let drain = async move {
                let _ = closing_rx.await;
                tokio::time::sleep(ADMIN_DRAIN).await;
            };
            tokio::select! {
                result = std::future::IntoFuture::into_future(serve) => {
                    if let Err(err) = result {
                        logger.error(MODULE, &format!("管理台异常退出: {err}"));
                    }
                }
                _ = drain => logger.warn(MODULE, "管理台仍有连接未关闭, 停止等待"),
            }
        });
        fields.push(("addr", addr));
        self.log_start_step("6/7 启动管理台 HTTP", fields);
        Ok(())
    }

    fn admin_router(&self) -> AxumRouter {
        let runtime = &self.runtime;
        let state = AdminState::with_shared(
            runtime.config_path.clone(),
            Arc::clone(&runtime.app_config),
        )
        .with_config_listener(Arc::new(LiveConfig {
            pipeline: Arc::clone(&runtime.pipeline),
            router: Arc::clone(runtime.pipeline.router()),
            logger: Arc::clone(&runtime.logger),
        }))
        .with_status(Arc::new(AdminStatus {
            started: runtime.started_at,
            config: Arc::clone(&runtime.config),
            gateway: Arc::clone(&runtime.gateway),
            registry: Arc::clone(runtime.pipeline.router().registry()),
        }))
        .with_stats(Arc::new(AdminStats {
            pipeline: Arc::clone(&runtime.pipeline),
            store: runtime.database.as_ref().map(|db| Arc::new(db.stats())),
        }))
        .with_plugins(Arc::new(
            AdminPlugins::new(
                Arc::clone(runtime.pipeline.router().registry()),
                Arc::clone(runtime.plugin_config()),
            )
            .with_app_config(
                Arc::clone(&runtime.app_config),
                runtime.config_path.clone(),
                Arc::clone(runtime.pipeline.router()),
            ),
        ))
        .with_live(Arc::clone(&runtime.events_bus))
        .with_logger(Arc::clone(&runtime.logger));
        let logs: Arc<dyn LogSource> = runtime.logger.clone();
        let mut state = state
            .with_logs(logs)
            .with_jobs(Arc::new(crate::jobs::AdminJobs::new(Arc::clone(
                &runtime.jobs,
            ))));
        if let Some(database) = runtime.database.as_ref() {
            let sender = Arc::new(crate::ports::TransportInboxSender::new(Arc::clone(
                runtime.pipeline.transport(),
            ))) as Arc<dyn aurorix_admin::InboxSender>;
            state = state
                .with_database(Arc::clone(database))
                .with_inbox(Arc::new(aurorix_admin::PgInbox::with_sender(
                    database.inbox(),
                    sender,
                )));
            if let Some(reload) = runtime.assets_reload.clone() {
                state = state.with_assets(Arc::new(
                    aurorix_admin::PgAssets::new(database.assets()).with_reload(reload),
                ));
            }
        }
        if let Some(remote) = runtime.panel_remote.clone() {
            let local: Arc<dyn aurorix_admin::MenuSource> = match runtime.database.as_ref() {
                Some(database) => Arc::new(aurorix_admin::PgMenu::new(database.panels())),
                None => Arc::new(aurorix_admin::EmptyMenu),
            };
            let menu = aurorix_admin::RemoteMenuSource::new(local)
                .with_remote(remote)
                .with_logger(Arc::clone(&runtime.logger));
            state = state.with_menu(Arc::new(menu));
        }
        let stop_tx = runtime.control_tx.clone();
        let restart_tx = runtime.control_tx.clone();
        let supervised = std::env::var(SUPERVISED_ENV).is_ok();
        let control = Control {
            supervised,
            restart: Some(Arc::new(move || {
                let _ = restart_tx.try_send(ControlCommand::Restart);
            }) as ControlAction),
            stop: Some(Arc::new(move || {
                let _ = stop_tx.try_send(ControlCommand::Stop);
            }) as ControlAction),
        };
        let state = state.with_control(control);
        aurorix_admin::spawn_live_tasks(state.clone(), Arc::clone(&runtime.events_bus));
        aurorix_admin::router(state)
    }

    fn spawn_jobs(&self) {
        let runtime = &self.runtime;
        let runner = crate::jobs::JobRunner::new(
            Arc::clone(&runtime.jobs),
            Arc::clone(runtime.pipeline.transport()),
            Arc::clone(&runtime.logger),
        );
        let handle = runner.spawn(runtime.stop_rx.clone());
        lock(&runtime.tasks).push(handle);
        let total = runtime.jobs.len();
        runtime.logger.info(
            MODULE,
            &format!("定时任务调度器已启动, 已注册 {total} 个任务"),
        );
    }

    fn spawn_memory_heartbeat(&self) {
        let runtime = Arc::clone(&self.runtime);
        let mut stop = runtime.stop_rx.clone();
        let logger = Arc::clone(&runtime.logger);
        runtime.spawn(async move {
            let mut ticker = tokio::time::interval(MEMORY_HEARTBEAT);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    _ = ticker.tick() => {
                        let pressure = aurorix_core::sys::check_memory_pressure();
                        if pressure.is_high() {
                            let report = aurorix_render::memory::emergency_reclaim();
                            logger.warn(
                                MODULE,
                                &format!(
                                    "内存{}水位, 已紧急回收 {} 项缓存, 当前常驻 {:.1} MB",
                                    pressure.label(),
                                    report.total(),
                                    aurorix_core::sys::resident_bytes() as f64 / 1048576.0
                                ),
                            );
                        }
                        aurorix_render::memory::purge_caches();
                        aurorix_core::sys::trim_memory();
                        let resident = aurorix_core::sys::resident_bytes();
                        logger.debug(
                            MODULE,
                            &format!(
                                "内存心跳, Skia 缓存已清理, 当前常驻 {:.1} MB",
                                resident as f64 / 1048576.0
                            ),
                        );
                    }
                }
            }
        });
    }

    fn spawn_event_loop(&self) {
        let runtime = Arc::clone(&self.runtime);
        let signal_handling = self.signal_handling;
        let events = lock(&runtime.events_rx).take();
        let controls = lock(&runtime.control_rx).take();
        let mut stop = runtime.stop_rx.clone();
        let done_tx = runtime.done_tx.clone();
        let loop_logger = Arc::clone(&runtime.logger);
        let loop_runtime = Arc::clone(&runtime);
        runtime.spawn(async move {
            let mut events = events;
            let mut controls = controls;
            let mut outcome = ControlCommand::Stop;
            let logger = Arc::clone(&loop_logger);
            let pool = crate::pool::WorkPool::from_hardware(Arc::clone(&logger));
            logger.info(
                MODULE,
                &format!(
                    "事件工作池就绪: 并发 {} 队列 {} 单任务超时 {} 秒",
                    pool.max(),
                    pool.queue(),
                    pool.timeout().as_secs()
                ),
            );
            let signal = async move {
                match signal_handling {
                    true => wait_for_shutdown_signal().await,
                    false => std::future::pending::<()>().await,
                }
            };
            tokio::pin!(signal);
            loop {
                let step = match events.as_mut() {
                    Some(receiver) => tokio::select! {
                        biased;
                        _ = &mut signal => LoopStep::Control(ControlCommand::Stop, "收到退出信号"),
                        _ = stop.changed() => LoopStep::Stop,
                        command = recv_control(&mut controls) => match command {
                            Some(command) => LoopStep::Control(command, "管理台控制指令"),
                            None => LoopStep::Stop,
                        },
                        event = receiver.recv() => match event {
                            Some(event) => LoopStep::Event(event),
                            None => LoopStep::Stop,
                        },
                    },
                    None => LoopStep::Stop,
                };
                match step {
                    LoopStep::Event(event) => {
                        let task = process_event(
                            Arc::clone(&loop_runtime.pipeline),
                            Arc::clone(&logger),
                            event,
                        );
                        pool.dispatch("事件处理", task).await;
                    }
                    LoopStep::Control(command, reason) => {
                        logger.info(MODULE, &format!("{reason}: {}", command.label()));
                        outcome = command;
                        break;
                    }
                    LoopStep::Stop => break,
                }
            }
            if let Some(receiver) = events.as_mut() {
                while let Ok(event) = receiver.try_recv() {
                    let task = process_event(
                        Arc::clone(&loop_runtime.pipeline),
                        Arc::clone(&logger),
                        event,
                    );
                    pool.dispatch("事件处理", task).await;
                }
            }
            let deadline = tokio::time::Instant::now() + DEFAULT_SHUTDOWN_TIMEOUT;
            while pool.in_flight() > 0 && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            loop_runtime.set_outcome(outcome);
            let _ = loop_runtime.stop_tx.send(true);
            let _ = done_tx.send(true);
        });
    }

    fn log_start_step(&self, step: &str, fields: Vec<(&str, String)>) {
        let mut map = BTreeMap::new();
        for (key, value) in fields {
            map.insert(key.to_string(), value);
        }
        self.runtime.logger.log(Level::Info, MODULE, step, map);
    }
}

#[cfg(unix)]
pub async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut interrupt = signal(SignalKind::interrupt()).ok();
    let mut terminate = signal(SignalKind::terminate()).ok();
    match (interrupt.as_mut(), terminate.as_mut()) {
        (Some(interrupt), Some(terminate)) => {
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
            }
        }
        (Some(interrupt), None) => {
            let _ = interrupt.recv().await;
        }
        (None, Some(terminate)) => {
            let _ = terminate.recv().await;
        }
        (None, None) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(not(unix))]
pub async fn wait_for_shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn recv_control(
    receiver: &mut Option<mpsc::Receiver<ControlCommand>>,
) -> Option<ControlCommand> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

enum LoopStep {
    Event(Arc<ProtocolEvent>),
    Control(ControlCommand, &'static str),
    Stop,
}

pub struct BotBuilder {
    config: BotConfig,
    app_config: Option<AppConfig>,
    fallback: FallbackPolicy,
    api: Option<Arc<BotApi>>,
    transport: Option<Arc<dyn ReplyTransport>>,
    tokens: Option<Arc<dyn TokenSource>>,
    gateway_transport: Option<Arc<dyn Transport>>,
    kv: Option<Arc<dyn KvBackend>>,
    inbox: Option<Arc<dyn InboxArchive>>,
    stats: Option<Arc<dyn StatsRecorder>>,
    renderer: Option<Arc<dyn Renderer>>,
    images: Option<Arc<dyn aurorix_api::ImageStore>>,
    footers: Option<Arc<aurorix_buttons::FooterHub>>,
    callbacks: Option<Arc<aurorix_plugin::CallbackBook>>,
    jobs: Option<Arc<aurorix_plugin::JobRegistry>>,
    registry: Option<Arc<CommandRegistry>>,
    database: Option<Arc<aurorix_db::Db>>,
    templates: Option<Arc<TemplateRegistry>>,
    plugin_dbs: Option<Arc<aurorix_db::PluginDbRegistry>>,
    plugin_assets: Option<Arc<aurorix_plugin::PluginAssetBook>>,
    template_override_dir: Option<String>,
    roles: Option<Arc<dyn RoleResolver>>,
    logger: Option<Arc<Logger>>,
    config_sinks: Vec<Arc<dyn PluginConfigurable>>,
    assets_reload: Option<Arc<dyn aurorix_admin::AssetReload>>,
    config_path: Option<PathBuf>,
    panel_remote: Option<Arc<dyn aurorix_admin::PanelRemote>>,
    signal_handling: bool,
    event_buffer: usize,
    dedup_ttl: Duration,
    instance_lock: bool,
}

impl BotBuilder {
    pub fn new(config: BotConfig) -> Self {
        Self {
            config,
            fallback: FallbackPolicy::Deny,
            api: None,
            transport: None,
            tokens: None,
            gateway_transport: None,
            kv: None,
            inbox: None,
            stats: None,
            renderer: None,
            images: None,
            footers: None,
            callbacks: None,
            jobs: None,
            registry: None,
            database: None,
            templates: None,
            plugin_dbs: None,
            plugin_assets: None,
            template_override_dir: None,
            roles: None,
            logger: None,
            config_sinks: Vec::new(),
            assets_reload: None,
            config_path: None,
            app_config: None,
            panel_remote: None,
            signal_handling: true,
            event_buffer: DEFAULT_EVENT_BUFFER,
            dedup_ttl: aurorix_api::DEFAULT_DEDUP_TTL,
            instance_lock: true,
        }
    }

    pub fn config(mut self, config: BotConfig) -> Self {
        self.config = config;
        self
    }

    pub fn allow_fallbacks(mut self) -> Self {
        self.fallback = FallbackPolicy::Memory;
        self
    }

    pub fn fallback_policy(mut self, policy: FallbackPolicy) -> Self {
        self.fallback = policy;
        self
    }

    pub fn with_transport(mut self, transport: Arc<dyn ReplyTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn with_api(mut self, api: Arc<BotApi>) -> Self {
        let reply: Arc<dyn ReplyTransport> = api.clone();
        let tokens: Arc<dyn TokenSource> = Arc::new(ApiTokens::new(Arc::clone(&api)));
        self.api = Some(api);
        self.with_transport(reply).with_tokens(tokens)
    }

    pub fn with_tokens(mut self, tokens: Arc<dyn TokenSource>) -> Self {
        self.tokens = Some(tokens);
        self
    }

    pub fn with_gateway_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.gateway_transport = Some(transport);
        self
    }

    pub fn with_kv(mut self, kv: Arc<dyn KvBackend>) -> Self {
        self.kv = Some(kv);
        self
    }

    pub fn with_inbox(mut self, inbox: Arc<dyn InboxArchive>) -> Self {
        self.inbox = Some(inbox);
        self
    }

    pub fn with_stats(mut self, stats: Arc<dyn StatsRecorder>) -> Self {
        self.stats = Some(stats);
        self
    }

    pub fn with_plugins(mut self, registry: Arc<CommandRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    pub fn with_database(mut self, database: Arc<aurorix_db::Db>) -> Self {
        self.database = Some(database);
        self
    }

    pub fn with_render(mut self, renderer: Arc<dyn Renderer>) -> Self {
        self.renderer = Some(renderer);
        self
    }
    pub fn with_images(mut self, images: Arc<dyn aurorix_api::ImageStore>) -> Self {
        self.images = Some(images);
        self
    }

    pub fn with_asset_reload(
        mut self,
        reload: Arc<dyn aurorix_admin::AssetReload>,
    ) -> Self {
        self.assets_reload = Some(reload);
        self
    }

    pub fn with_footers(mut self, footers: Arc<aurorix_buttons::FooterHub>) -> Self {
        self.footers = Some(footers);
        self
    }

    pub fn with_callbacks(mut self, callbacks: Arc<aurorix_plugin::CallbackBook>) -> Self {
        self.callbacks = Some(callbacks);
        self
    }

    pub fn with_jobs(mut self, jobs: Arc<aurorix_plugin::JobRegistry>) -> Self {
        self.jobs = Some(jobs);
        self
    }

    pub fn with_logger(mut self, logger: Arc<Logger>) -> Self {
        self.logger = Some(logger);
        self
    }

    pub fn with_templates(mut self, templates: Arc<TemplateRegistry>) -> Self {
        self.templates = Some(templates);
        self
    }
    pub fn with_plugin_dbs(mut self, registry: Arc<aurorix_db::PluginDbRegistry>) -> Self {
        self.plugin_dbs = Some(registry);
        self
    }

    pub fn with_plugin_assets(mut self, book: Arc<aurorix_plugin::PluginAssetBook>) -> Self {
        self.plugin_assets = Some(book);
        self
    }

    pub fn with_template_override(mut self, dir: impl Into<String>) -> Self {
        self.template_override_dir = Some(dir.into());
        self
    }

    pub fn with_roles(mut self, roles: Arc<dyn RoleResolver>) -> Self {
        self.roles = Some(roles);
        self
    }

    pub fn with_plugin_config(mut self, sink: Arc<dyn PluginConfigurable>) -> Self {
        self.config_sinks.push(sink);
        self
    }

    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    pub fn with_app_config(mut self, config: AppConfig) -> Self {
        self.app_config = Some(config);
        self
    }

    pub fn with_panel_remote(mut self, remote: Arc<dyn aurorix_admin::PanelRemote>) -> Self {
        self.panel_remote = Some(remote);
        self
    }

    pub fn with_signal_handling(mut self, enabled: bool) -> Self {
        self.signal_handling = enabled;
        self
    }

    pub fn with_event_buffer(mut self, capacity: usize) -> Self {
        self.event_buffer = capacity.max(1);
        self
    }

    pub fn with_dedup_ttl(mut self, ttl: Duration) -> Self {
        self.dedup_ttl = ttl;
        self
    }

    pub fn with_instance_lock(mut self, enabled: bool) -> Self {
        self.instance_lock = enabled;
        self
    }

    pub fn build(self) -> Result<Bot, BotError> {
        self.config.validate()?;
        let level = self.config.level()?;
        let config = Arc::new(self.config);
        let config_path = match &self.config_path {
            Some(path) => path.clone(),
            None => PathBuf::from(aurorix_db::CONFIG_FILE),
        };
        let mut app_config = match self.app_config {
            Some(app) => app,
            None => match AppConfig::load(&config_path) {
                Ok(app) => app,
                Err(_) => app_config_snapshot(&config),
            },
        };
        app_config.normalize();
        let logger = self.logger.unwrap_or_else(|| {
            Logger::new(Options::default().with_level(level).with_console(true))
        });
        let transport = match self.transport {
            Some(transport) => transport,
            None => {
                return Err(BotError::missing(
                    "ReplyTransport",
                    "发送通道未注入: 事件就算命中指令也发不出去",
                ))
            }
        };
        let tokens = match self.tokens {
            Some(tokens) => tokens,
            None => {
                return Err(BotError::missing(
                    "TokenSource",
                    "访问令牌来源未注入: 网关拿不到 access token",
                ))
            }
        };
        let registry = match self.registry {
            Some(registry) => registry,
            None => {
                return Err(BotError::missing(
                    "CommandRegistry",
                    "指令注册表未注入: 没有任何指令可以分发",
                ))
            }
        };
        let stats = match self.stats {
            Some(stats) => Some(stats),
            None => match self.fallback {
                FallbackPolicy::Memory => {
                    logger.warn(MODULE, "未注入 StatsRecorder, 回退到内存统计(重启即丢)");
                    Some(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
                }
                FallbackPolicy::Deny => {
                    return Err(BotError::missing(
                        "StatsRecorder",
                        "统计没有落点: 收到/发出计数无处可写",
                    ))
                }
            },
        };
        let kv = match self.kv {
            Some(kv) => Some(kv),
            None => match self.fallback {
                FallbackPolicy::Memory => {
                    logger.warn(MODULE, "未注入 KvBackend, 回退到内存 KV(重启即丢)");
                    Some(Arc::new(MemoryKv::new()) as Arc<dyn KvBackend>)
                }
                FallbackPolicy::Deny => {
                    logger.info(MODULE, "未注入 KvBackend, 插件配置存储不启用");
                    None
                }
            },
        };
        let inbox = match self.inbox {
            Some(inbox) => Some(inbox),
            None => {
                logger.info(MODULE, "未注入 InboxArchive, 收件箱归档不启用");
                None
            }
        };
        let renderer = match self.renderer {
            Some(renderer) => Some(renderer),
            None => {
                logger.info(MODULE, "未注入 Renderer, 卡片回复走纯文本");
                None
            }
        };
        let images = self.images;
        let jobs = match self.jobs {
            Some(jobs) => jobs,
            None => Arc::new(aurorix_plugin::JobRegistry::new()),
        };
        let footers = match self.footers {
            Some(footers) => footers,
            None => Arc::new(footer_hub(&config)),
        };
        let keyboard_font = Arc::clone(&config);
        aurorix_buttons::set_default_font_size(Some(Arc::new(move || {
            keyboard_font
                .keyboard_font_size
                .clone()
                .unwrap_or_default()
        })));
        let templates = self
            .templates
            .unwrap_or_else(|| Arc::new(TemplateRegistry::new()));
        let template_reload = match self.template_override_dir {
            Some(dir) => {
                let overrides = Arc::new(aurorix_template::Overrides::with_global(
                    Arc::clone(&templates),
                    dir,
                ));
                match overrides.load() {
                    Ok(()) => {
                        logger.info(MODULE, &format!("模板覆盖目录已启用: {}", overrides.root()))
                    }
                    Err(error) => logger.warn(MODULE, &format!("模板覆盖目录读取失败: {error}")),
                }
                Some(overrides.spawn_reloader())
            }
            None => None,
        };
        let mut router = Router::new(Arc::clone(&registry))
            .with_templates(templates)
            .with_ban(Arc::new(aurorix_plugin::BanList::new()));
        if let Some(dbs) = self.plugin_dbs {
            router = router.with_plugin_db_registry(dbs);
        }
        if let Some(book) = self.plugin_assets {
            router = router.with_plugin_assets(book);
        }
        if let Some(roles) = self.roles {
            router = router.with_roles(roles);
        }
        let router = Arc::new(router);
        let maintenance = maintenance_text(&app_config);
        if let Some(text) = &maintenance {
            router.set_global_maintenance(PluginMaintenance::on(text.clone()));
        }
        let events_bus = aurorix_admin::LiveBus::shared();
        let deps = PipelineDeps {
            config: Arc::clone(&config),
            router,
            transport,
            logger: Arc::clone(&logger),
            dedup_ttl: self.dedup_ttl,
            kv,
            inbox,
            stats,
            renderer,
            images,
            footers: Some(footers),
            callbacks: self.callbacks,
            maintenance,
            live: Some(Arc::clone(&events_bus)),
        };
        let pipeline = Pipeline::new(deps);
        let (events_tx, events_rx) = mpsc::channel::<Arc<ProtocolEvent>>(self.event_buffer);
        let sink = Arc::new(ChannelSink {
            tx: events_tx,
            logger: Arc::clone(&logger),
            dropped: AtomicU64::new(0),
        });
        let gateway_transport = self
            .gateway_transport
            .unwrap_or_else(|| Arc::new(WebSocketTransport::new()));
        let gateway = GatewayClient::with_transport(
            config.gateway_config(),
            tokens,
            Arc::clone(&sink) as Arc<dyn EventSink>,
            gateway_transport,
        )
        .with_logger(Arc::clone(&logger));
        let gateway = match (config.gateway_url.is_none(), self.api.as_ref()) {
            (true, Some(api)) => gateway.with_source(Arc::new(OfficialGatewaySource::new(
                Arc::clone(api),
                Arc::clone(&logger),
            ))),
            _ => gateway,
        };
        let gateway = Arc::new(gateway);
        let (control_tx, control_rx) = mpsc::channel(CONTROL_BUFFER);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (done_tx, done_rx) = watch::channel(false);
        let plugin_config = Arc::new(PluginConfigStore::new(
            Some(config_path.clone()),
            config.plugin_settings.clone(),
        ));
        for sink in &self.config_sinks {
            plugin_config.attach(Arc::clone(sink));
        }
        let runtime = Arc::new(Runtime {
            config,
            app_config: Arc::new(Mutex::new(app_config)),
            config_path,
            logger,
            pipeline,
            gateway,
            control_tx,
            control_rx: Mutex::new(Some(control_rx)),
            events_rx: Mutex::new(Some(events_rx)),
            stop_tx,
            stop_rx,
            tasks: Mutex::new(Vec::new()),
            live: Arc::new(AtomicUsize::new(0)),
            gateway_slot: AtomicBool::new(false),
            outcome: Mutex::new(None),
            done_tx,
            done_rx,
            started_at: Instant::now(),
            config_failures: Mutex::new(Vec::new()),
            plugin_config,
            panel_remote: self.panel_remote.clone(),
            sink: Arc::clone(&sink) as Arc<dyn EventSink>,
            jobs,
            database: self.database,
            assets_reload: self.assets_reload,
            instance_lock: Mutex::new(None),
            report: Mutex::new(None),
            template_reload,
            events_bus,
        });
        Ok(Bot {
            runtime,
            config_sinks: self.config_sinks,
            signal_handling: self.signal_handling,
            started: AtomicBool::new(false),
            instance_lock: self.instance_lock,
        })
    }
}

fn yes_no(value: bool) -> String {
    match value {
        true => String::from("已注入"),
        false => String::from("未注入"),
    }
}

fn footer_hub(config: &Arc<BotConfig>) -> aurorix_buttons::FooterHub {
    use aurorix_buttons::FooterScope;
    let hub = aurorix_buttons::FooterHub::new();
    let scopes = [
        (FooterScope::Global, None),
        (FooterScope::SceneGroup, Some(aurorix_core::Scene::Group)),
        (FooterScope::SceneC2c, Some(aurorix_core::Scene::C2c)),
        (
            FooterScope::SceneChannel,
            Some(aurorix_core::Scene::Channel),
        ),
        (FooterScope::SceneDm, Some(aurorix_core::Scene::Dm)),
    ];
    for (scope, scene) in scopes {
        let config = Arc::clone(config);
        let source = move || match scene {
            Some(scene) => config.footer_scene_lines(scene),
            None => config.footer_global_lines(),
        };
        hub.set_source(
            scope,
            Some(Arc::new(source) as Arc<dyn Fn() -> Vec<String> + Send + Sync>),
        );
    }
    let welcome = Arc::clone(config);
    hub.set_source(
        FooterScope::NoticeWelcome,
        Some(Arc::new(move || welcome.welcome_button_lines())
            as Arc<dyn Fn() -> Vec<String> + Send + Sync>),
    );
    let farewell = Arc::clone(config);
    hub.set_source(
        FooterScope::NoticeFarewell,
        Some(Arc::new(move || farewell.farewell_button_lines())
            as Arc<dyn Fn() -> Vec<String> + Send + Sync>),
    );
    hub
}

fn maintenance_text(config: &AppConfig) -> Option<String> {
    match config.bot_maintenance.unwrap_or(false) {
        true => Some(config.maintenance_message().to_string()),
        false => None,
    }
}

fn apply_access(router: &Router, config: &AppConfig) {
    for (plugin, access) in config.plugin_access.iter().flatten() {
        router.access().set(plugin, to_plugin_access(access));
    }
}

fn apply_maintenance(pipeline: &Pipeline, router: &Router, config: &AppConfig) {
    let message = maintenance_text(config);
    pipeline.set_maintenance(message.clone());
    router.set_global_maintenance(to_plugin_maintenance(&aurorix_db::Maintenance {
        enabled: message.is_some(),
        message,
    }));
}

fn apply_log_level(logger: &Logger, config: &AppConfig) {
    let Some(text) = config.log_level.as_deref() else {
        return;
    };
    if let Err(error) = logger.set_level_text(text) {
        logger.warn(
            MODULE,
            &format!("日志级别 {text} 无法应用, 保持原级别: {error}"),
        );
    }
}

struct LiveConfig {
    pipeline: Arc<Pipeline>,
    router: Arc<Router>,
    logger: Arc<Logger>,
}

impl aurorix_admin::ConfigListener for LiveConfig {
    fn applied(&self, config: &AppConfig) {
        apply_maintenance(&self.pipeline, &self.router, config);
        apply_access(&self.router, config);
        apply_log_level(&self.logger, config);
    }
}

fn app_config_snapshot(config: &BotConfig) -> AppConfig {
    let mut app = AppConfig {
        port: config.admin_port,
        listen_addr: Some(config.admin_listen.clone()),
        appid: config.appid.clone(),
        secret: config.secret.clone(),
        proxy: config.proxy_api.clone(),
        database: config.database.clone(),
        admin_password: config.admin_password.clone(),
        protocol: Some(config.protocol.clone()),
        intents: Some(config.intents.clone()),
        gateway_url: config.gateway_url.clone(),
        log_level: Some(config.log_level.clone()),
        prefixes: Some(config.prefixes.clone()),
        welcome_enabled: config.welcome_enabled,
        welcome_messages: config.welcome_messages.clone(),
        welcome_extra: config.welcome_extra.clone(),
        welcome_buttons: config.welcome_buttons.clone(),
        farewell_enabled: config.farewell_enabled,
        farewell_messages: config.farewell_messages.clone(),
        farewell_extra: config.farewell_extra.clone(),
        farewell_buttons: config.farewell_buttons.clone(),
        panel_sync: Some(config.panel_sync),
        bot_maintenance: Some(config.maintenance.is_some()),
        bot_maintenance_message: config.maintenance.clone(),
        global_markdown: Some(config.global_markdown),
        retry_when: Some(config.retry_when.clone()),
        upload_threshold: Some(config.upload_threshold as i64),
        image_limits_enabled: config.image_limits_enabled,
        image_max_width: config.image_max_width,
        image_max_height: config.image_max_height,
        keyboard_font_size: config.keyboard_font_size.clone(),
        footer_buttons_enabled: config.footer_buttons_enabled,
        footer_buttons: config.footer_buttons.clone(),
        footer_buttons_group: config.footer_buttons_group.clone(),
        footer_buttons_c2c: config.footer_buttons_c2c.clone(),
        footer_buttons_channel: config.footer_buttons_channel.clone(),
        footer_buttons_dm: config.footer_buttons_dm.clone(),
        plugin_settings: Some(config.plugin_settings.clone()),
        plugin_access: Some(config.plugin_access.clone()),
        ..AppConfig::default()
    };
    app.normalize();
    app
}

fn to_plugin_ban(entry: aurorix_db::BanEntry) -> aurorix_plugin::BanEntry {
    aurorix_plugin::BanEntry {
        openid: entry.openid,
        message: entry.message,
        note: entry.note,
        at: entry.at,
    }
}

fn to_plugin_access(config: &aurorix_db::AccessConfig) -> PluginAccessConfig {
    let mut commands = BTreeMap::new();
    for (path, rule) in config.commands.iter().flatten() {
        commands.insert(path.clone(), to_plugin_rule(rule));
    }
    PluginAccessConfig {
        default: to_plugin_rule(&config.default),
        commands,
        disabled: config.disabled,
        maintenance: to_plugin_maintenance(&config.maintenance),
        deny: to_plugin_deny(config.deny.as_ref()),
    }
}

fn to_plugin_rule(rule: &aurorix_db::AccessRule) -> PluginAccessRule {
    PluginAccessRule {
        mode: AccessMode::parse(&rule.mode).unwrap_or_default(),
        users: rule.users.clone().unwrap_or_default(),
        groups: rule.groups.clone().unwrap_or_default(),
        role: rule.role,
        maintenance: to_plugin_maintenance(rule.maintenance.as_ref().unwrap_or(
            &aurorix_db::Maintenance {
                enabled: false,
                message: None,
            },
        )),
        deny: to_plugin_deny(rule.deny.as_ref()),
    }
}

fn to_plugin_maintenance(maintenance: &aurorix_db::Maintenance) -> PluginMaintenance {
    match maintenance.enabled {
        true => PluginMaintenance::on(maintenance.message.clone().unwrap_or_default()),
        false => PluginMaintenance::default(),
    }
}

fn to_plugin_deny(deny: Option<&aurorix_db::Deny>) -> PluginDeny {
    match deny {
        Some(deny) => PluginDeny {
            reply: deny.reply,
            message: deny.message.clone().unwrap_or_default(),
        },
        None => PluginDeny::default(),
    }
}

struct AdminStatus {
    started: Instant,
    config: Arc<BotConfig>,
    gateway: Arc<GatewayClient>,
    registry: Arc<CommandRegistry>,
}

impl StatusSource for AdminStatus {
    fn snapshot(&self) -> StatusView {
        let stats = self.gateway.stats();
        StatusView {
            uptime_seconds: self.started.elapsed().as_secs(),
            version: String::from(env!("CARGO_PKG_VERSION")),
            protocol: self.config.protocol.clone(),
            port: self.config.admin_port,
            tasks: stats.live_tasks as u64,
            gateway: GatewayStatus {
                connected: stats.ready,
                session_id: stats.session_id.clone().unwrap_or_default(),
                seq: stats.seq,
                heartbeat_ack: stats.heartbeat_interval_ms.is_some(),
                since_ms: stats.connected_since_ms,
                bot_id: stats.bot_id.clone().unwrap_or_default(),
                bot_name: stats.bot_name.clone().unwrap_or_default(),
                shard_id: stats.shard[0],
                shards: stats.shard[1],
            },
            counts: AdminCounts {
                plugins: self.registry.plugins().len() as u64,
                commands: self.registry.command_count() as u64,
                jobs: 0,
                templates: 0,
            },
        }
    }
}

struct AdminStats {
    pipeline: Arc<Pipeline>,
    store: Option<Arc<aurorix_db::StatsStore>>,
}

impl AdminStats {
    fn counters_view(&self) -> aurorix_db::StatsView {
        let counters = self.pipeline.counters();
        aurorix_db::StatsView {
            total_recv: counters.received,
            total_sent: counters.replied,
            ..aurorix_db::StatsView::default()
        }
    }
}

impl StatsSource for AdminStats {
    fn view(&self) -> aurorix_db::StatsView {
        self.counters_view()
    }

    fn view_async(&self) -> aurorix_admin::SourceFuture<'_, aurorix_db::StatsView> {
        Box::pin(async move {
            let Some(store) = &self.store else {
                return self.counters_view();
            };
            match store.full_view().await {
                Ok(view) => view,
                Err(error) => {
                    self.pipeline.logger().warn(
                        crate::pipeline::MODULE,
                        &format!("统计快照读取失败, 回退进程内计数: {error}"),
                    );
                    self.counters_view()
                }
            }
        })
    }
}

#[derive(Clone)]
pub struct AdminPlugins {
    registry: Arc<CommandRegistry>,
    config: Arc<PluginConfigStore>,
    app: Option<SharedApp>,
}

#[derive(Clone)]
struct SharedApp {
    config: Arc<Mutex<AppConfig>>,
    path: PathBuf,
    router: Arc<Router>,
}

impl AdminPlugins {
    pub fn new(registry: Arc<CommandRegistry>, config: Arc<PluginConfigStore>) -> Self {
        Self {
            registry,
            config,
            app: None,
        }
    }

    pub fn with_app_config(
        mut self,
        config: Arc<Mutex<AppConfig>>,
        path: PathBuf,
        router: Arc<Router>,
    ) -> Self {
        self.app = Some(SharedApp {
            config,
            path,
            router,
        });
        self
    }

    fn access_of(&self, plugin_id: &str) -> serde_json::Value {
        let Some(app) = &self.app else {
            return aurorix_admin::default_access();
        };
        let access = lock(&app.config).plugin_access_for(plugin_id);
        serde_json::to_value(access).unwrap_or_else(|_| aurorix_admin::default_access())
    }
    fn values_of(&self, slot: &aurorix_plugin::PluginSlot) -> serde_json::Value {
        let stored = self.config.values(&slot.id);
        let values = aurorix_plugin::resumed_values(&slot.config, &stored);
        serde_json::to_value(values).unwrap_or_else(|_| aurorix_admin::empty_object())
    }
}

impl PluginSource for AdminPlugins {
    fn plugins(&self) -> Vec<PluginInfo> {
        self.registry
            .plugins()
            .into_iter()
            .map(|slot| {
                let values = self.values_of(&slot);
                let fields = aurorix_plugin::fields_json(&slot.config);
                let access = self.access_of(&slot.id);
                PluginInfo {
                    description: slot.description,
                    commands: self
                        .registry
                        .index()
                        .command_paths(&slot.id)
                        .into_iter()
                        .collect(),
                    values,
                    access,
                    id: slot.id,
                    name: slot.name,
                    version: String::from(env!("CARGO_PKG_VERSION")),
                    enabled: true,
                    fields,
                }
            })
            .collect()
    }

    fn save_config(
        &self,
        id: &str,
        values: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), PluginError> {
        let Some(slot) = self.registry.plugin(id) else {
            return Err(PluginError::NotFound(format!("插件 {id} 不存在")));
        };
        let incoming: BTreeMap<String, serde_json::Value> = values.into_iter().collect();
        let normalized = aurorix_plugin::normalize_values(&slot.config, &incoming)
            .map_err(PluginError::BadRequest)?;
        let mut merged = aurorix_plugin::resumed_values(&slot.config, &self.config.values(id));
        for (key, value) in normalized {
            merged.insert(key, value);
        }
        self.config.save(id, merged).map_err(PluginError::Server)
    }

    fn save_access(&self, id: &str, access: serde_json::Value) -> Result<(), PluginError> {
        let Some(app) = &self.app else {
            return Err(PluginError::Server(String::from(
                "配置来源未注入, 访问控制无法落盘",
            )));
        };
        if self.registry.plugin(id).is_none() {
            return Err(PluginError::NotFound(format!("插件 {id} 不存在")));
        }
        let parsed: aurorix_db::AccessConfig = serde_json::from_value(access)
            .map_err(|error| PluginError::BadRequest(format!("访问控制格式不正确: {error}")))?;
        {
            let mut config = lock(&app.config);
            config
                .save_plugin_access(&app.path, id, parsed.clone())
                .map_err(|error| PluginError::Server(error.to_string()))?;
        }
        app.router.access().set(id, to_plugin_access(&parsed));
        Ok(())
    }
}

pub fn role_label(role: Option<RoleRequired>) -> &'static str {
    match role {
        Some(role) => role.as_str(),
        None => "unknown",
    }
}

fn blocked_by_maintenance(pipeline: &Pipeline, event: &ProtocolEvent) -> bool {
    if pipeline.maintenance().is_none() {
        return false;
    }
    crate::event::category(event.event_type.as_deref()) != EventCategory::Message
}

async fn process_event(pipeline: Arc<Pipeline>, logger: Arc<Logger>, event: Arc<ProtocolEvent>) {
    if blocked_by_maintenance(&pipeline, &event) {
        logger.debug(
            MODULE,
            &format!(
                "全局维护中, 事件已拦截: {}",
                event.event_type.clone().unwrap_or_default()
            ),
        );
        return;
    }
    let report = pipeline.handle(&event).await;
    if report.category != EventCategory::Message {
        return;
    }
    if report.outcome == crate::pipeline::Outcome::Duplicate {
        return;
    }
    logger.log(
        Level::Debug,
        crate::pipeline::MODULE,
        "事件处理完成",
        BTreeMap::from([
            (
                String::from("outcome"),
                report.outcome.label().to_string(),
            ),
            (
                String::from("stage"),
                report
                    .stage
                    .map(|stage| stage.label().to_string())
                    .unwrap_or_default(),
            ),
            (String::from("sent"), report.sent.to_string()),
        ]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurorix_log::Options;
    use serde_json::json;

    fn sample_event(event_type: &str) -> ProtocolEvent {
        ProtocolEvent {
            id: Some(String::from("evt-1")),
            op: 0,
            event_type: Some(event_type.to_string()),
            seq: Some(1),
            data: json!({ "id": "m1", "content": "hi" }),
            raw: String::new(),
        }
    }

    fn quiet_logger() -> Arc<Logger> {
        Logger::new(Options::default().with_console(false))
    }

    fn messages(logger: &Logger) -> Vec<String> {
        logger
            .recent(32)
            .iter()
            .map(|record| record.msg.clone())
            .collect()
    }

    #[tokio::test]
    async fn channel_sink_counts_dropped_events_starting_from_one() {
        let logger = quiet_logger();
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let sink = ChannelSink {
            tx,
            logger: Arc::clone(&logger),
            dropped: AtomicU64::new(0),
        };
        sink.dispatch(sample_event("GROUP_AT_MESSAGE_CREATE"));
        sink.dispatch(sample_event("GROUP_AT_MESSAGE_CREATE"));
        let seen = messages(&logger);
        assert!(
            seen.iter().any(|line| line.contains("累计 1 条")),
            "第一次丢弃要报累计 1 条: {seen:?}"
        );
        assert!(
            seen.iter().any(|line| line.contains("累计 2 条")),
            "第二次丢弃要报累计 2 条: {seen:?}"
        );
    }

    #[tokio::test]
    async fn channel_sink_keeps_sending_into_a_live_queue() {
        let logger = quiet_logger();
        let (tx, mut rx) = mpsc::channel(4);
        let sink = ChannelSink {
            tx,
            logger,
            dropped: AtomicU64::new(0),
        };
        sink.dispatch(sample_event("GROUP_AT_MESSAGE_CREATE"));
        let received = rx.recv().await.expect("事件要进队列");
        assert_eq!(received.id.as_deref(), Some("evt-1"));
    }

    #[test]
    fn plugin_access_is_saved_and_reloaded_from_the_config_file() {
        let dir = std::env::temp_dir().join(format!(
            "aurorix-bot-access-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"appid":"a","secret":"s","admin_password":"p"}"#)
            .expect("写配置");
        let registry = Arc::new(CommandRegistry::new());
        let registered = registry.register_plugin(
            aurorix_plugin::PluginCommands::new("demo", "示例插件").command(
                aurorix_plugin::CommandSpec::new("echo").on_sync(
                    |_invocation: &aurorix_plugin::Invocation| Ok(None),
                ),
            ),
        );
        assert!(registered.is_empty(), "示例插件不该有冲突警告");
        let router = Arc::new(Router::new(Arc::clone(&registry)));
        let app = Arc::new(Mutex::new(AppConfig::load(&path).expect("读配置")));
        let plugins = AdminPlugins::new(
            Arc::clone(&registry),
            Arc::new(PluginConfigStore::new(Some(path.clone()), BTreeMap::new())),
        )
        .with_app_config(Arc::clone(&app), path.clone(), Arc::clone(&router));
        plugins
            .save_access(
                "demo",
                json!({
                    "default": {
                        "mode": "off",
                        "users": [],
                        "groups": [],
                        "role": "",
                        "maintenance": { "enabled": false, "message": "" },
                        "deny": {}
                    },
                    "commands": {},
                    "disabled": false,
                    "maintenance": { "enabled": true, "message": "插件维护中" }
                }),
            )
            .expect("保存访问控制");
        let reloaded = AppConfig::load(&path).expect("重新加载");
        let stored = reloaded.plugin_access_for("demo");
        assert!(stored.maintenance.enabled);
        assert_eq!(stored.maintenance.message.as_deref(), Some("插件维护中"));
        assert!(
            router.access().plugin_maintenance("demo").enabled,
            "保存后要立刻生效"
        );
        let shown = plugins.plugin("demo").expect("插件存在").access;
        assert_eq!(shown["maintenance"]["enabled"], json!(true));
        std::fs::remove_dir_all(&dir).ok();
    }
}
