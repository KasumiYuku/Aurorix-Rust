use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use aurorix_db::AppConfig;
use aurorix_log::{Logger, Options};

use crate::adapters::{PgAssets, PgBans, PgInbox, PgMenu};
use crate::assets::{AssetSource, EmptyAssets};
use crate::auth::{SessionStore, DEFAULT_TOKEN_TTL_SECONDS};
use crate::bans::{BanSource, EmptyBans};
use crate::inbox::{EmptyInbox, InboxSource};
use crate::jobs::{EmptyJobs, JobSource};
use crate::live::LiveBus;
use crate::menu::{EmptyMenu, MenuSource};
use crate::profile::{EmptyProfile, ProfileSource};
use crate::sources::{
    ConfigListener, EmptyLogs, EmptyPlugins, EmptyStats, EmptyStatus, LogSource, PluginSource,
    StatsSource, StatusSource,
};
use crate::system::Control;
use crate::webui::{GlobalWebUis, WebUiSource};

#[derive(Clone)]
pub struct AdminState {
    pub config: Arc<Mutex<AppConfig>>,
    pub config_path: PathBuf,
    pub config_listener: Option<Arc<dyn ConfigListener>>,
    pub sessions: Arc<SessionStore>,
    pub status: Arc<dyn StatusSource>,
    pub stats: Arc<dyn StatsSource>,
    pub logs: Arc<dyn LogSource>,
    pub plugins: Arc<dyn PluginSource>,
    pub menu: Arc<dyn MenuSource>,
    pub bans: Arc<dyn BanSource>,
    pub inbox: Arc<dyn InboxSource>,
    pub jobs: Arc<dyn JobSource>,
    pub live: Arc<LiveBus>,
    pub assets: Arc<dyn AssetSource>,
    pub profile: Arc<dyn ProfileSource>,
    pub webui: Arc<dyn WebUiSource>,
    pub control: Control,
    pub logger: Arc<Logger>,
}

impl AdminState {
    pub fn new(config_path: impl AsRef<Path>, config: AppConfig) -> Self {
        Self::with_shared(config_path, Arc::new(Mutex::new(config)))
    }

    pub fn with_shared(config_path: impl AsRef<Path>, config: Arc<Mutex<AppConfig>>) -> Self {
        Self {
            config,
            config_path: config_path.as_ref().to_path_buf(),
            config_listener: None,
            sessions: Arc::new(SessionStore::with_ttl_seconds(DEFAULT_TOKEN_TTL_SECONDS)),
            status: Arc::new(EmptyStatus),
            stats: Arc::new(EmptyStats),
            logs: Arc::new(EmptyLogs),
            plugins: Arc::new(EmptyPlugins),
            menu: Arc::new(EmptyMenu),
            bans: Arc::new(EmptyBans),
            inbox: Arc::new(EmptyInbox),
            jobs: Arc::new(EmptyJobs),
            live: LiveBus::shared(),
            assets: Arc::new(EmptyAssets),
            profile: Arc::new(EmptyProfile),
            webui: Arc::new(GlobalWebUis),
            control: Control::default(),
            logger: Logger::new(Options::default().with_console(false)),
        }
    }

    pub fn with_config_listener(mut self, listener: Arc<dyn ConfigListener>) -> Self {
        self.config_listener = Some(listener);
        self
    }

    pub fn with_status(mut self, status: Arc<dyn StatusSource>) -> Self {
        self.status = status;
        self
    }

    pub fn with_stats(mut self, stats: Arc<dyn StatsSource>) -> Self {
        self.stats = stats;
        self
    }

    pub fn with_logs(mut self, logs: Arc<dyn LogSource>) -> Self {
        self.logs = logs;
        self
    }

    pub fn with_plugins(mut self, plugins: Arc<dyn PluginSource>) -> Self {
        self.plugins = plugins;
        self
    }

    pub fn with_menu(mut self, menu: Arc<dyn MenuSource>) -> Self {
        self.menu = menu;
        self
    }

    pub fn with_bans(mut self, bans: Arc<dyn BanSource>) -> Self {
        self.bans = bans;
        self
    }

    pub fn with_inbox(mut self, inbox: Arc<dyn InboxSource>) -> Self {
        self.inbox = inbox;
        self
    }

    pub fn with_jobs(mut self, jobs: Arc<dyn JobSource>) -> Self {
        self.jobs = jobs;
        self
    }

    pub fn with_assets(mut self, assets: Arc<dyn AssetSource>) -> Self {
        self.assets = assets;
        self
    }

    pub fn with_profile(mut self, profile: Arc<dyn ProfileSource>) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_webui(mut self, webui: Arc<dyn WebUiSource>) -> Self {
        self.webui = webui;
        self
    }
    pub fn with_database(self, db: Arc<aurorix_db::Db>) -> Self {
        self.with_bans(Arc::new(PgBans::new(db.bans())))
            .with_menu(Arc::new(PgMenu::new(db.panels())))
            .with_assets(Arc::new(PgAssets::new(db.assets())))
            .with_inbox(Arc::new(PgInbox::new(db.inbox())))
    }

    pub fn with_control(mut self, control: Control) -> Self {
        self.control = control;
        self
    }

    pub fn with_live(mut self, live: Arc<LiveBus>) -> Self {
        self.live = live;
        self
    }

    pub fn with_logger(mut self, logger: Arc<Logger>) -> Self {
        self.logger = logger;
        self
    }

    pub fn with_token_ttl(mut self, ttl: Duration) -> Self {
        self.sessions = Arc::new(SessionStore::new(ttl));
        self
    }

    pub fn with_token_ttl_seconds(self, seconds: u64) -> Self {
        self.with_token_ttl(Duration::from_secs(seconds))
    }

    pub fn lock_config(&self) -> MutexGuard<'_, AppConfig> {
        self.config.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub fn config_snapshot(&self) -> AppConfig {
        self.lock_config().clone()
    }

    pub fn password(&self) -> String {
        self.lock_config().admin_password.clone()
    }

    pub fn config_file(&self) -> &Path {
        &self.config_path
    }
}
