use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aurorix_api::BotApi;
use aurorix_assets::{
    set_official_upload, AssetError, OfficialUploadFn, OfficialUploadRequest,
};
use aurorix_bot::{
    ApiPanelRemote, ApiTokens, Bot, BotConfig, DbStats, HostImageStore, HostReload, InboxArchive,
    KvBackend, Renderer, Scene, SchedulerRenderer,
};
use aurorix_db::{AppConfig, Db, InboxStore, RegistryConfig, CONFIG_FILE};
use aurorix_log::{ColorMode, FileOptions, Level, Logger, Options};
use aurorix_plugin::{CommandRegistry, JobRegistry, PluginAssetBook, PluginAssets};
use aurorix_render::{RenderContext, RenderScheduler, SchedulerConfig, SkiaBackend};
mod wiring;

const MODULE: &str = "aurorix";

const ASSETS_ROOT: &str = "assets";

const TEMPLATE_OVERRIDE_DIR: &str = "templates/override";

const PLUGIN_FONT_CAPACITY: usize = 64;

const CONFIG_PATH_ENV: &str = "AURORIX_CONFIG";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (app_config, config) = load_config()?;
    let profile = *aurorix_core::HardwareProfile::cached();
    let logger = Logger::new(
        Options::default()
            .with_level(Level::parse(&config.log_level).unwrap_or(Level::Info))
            .with_console(true)
            .with_color(ColorMode::Auto)
            .with_capacity(profile.log_ring_capacity())
            .with_byte_budget(profile.log_ring_bytes())
            .with_file(FileOptions::default()),
    );
    logger.info(MODULE, &format!("自适应硬件档位: {}", profile.summary()));
    let database = config.database.clone();
    let db = Arc::new(Db::connect(&database).await?);
    db.ensure_schema().await?;
    logger.info(MODULE, "数据库已连接, 表结构已核对");

    let config_cache = db.config_cache();
    let cached = config_cache.load().await.unwrap_or_default();
    logger.info(MODULE, &format!("全局配置已装入 L1 缓存: {cached} 项"));

    let templates = Arc::new(aurorix_template::Registry::new());
    let builtin_templates = aurorix_template::register_builtin_templates(&templates);
    logger.info(MODULE, &format!("已注册内置模板: {builtin_templates} 个"));
    let mut book = PluginAssetBook::new();
    let mut loaders = Vec::new();
    let registry = Arc::new(CommandRegistry::new());
    let plugin_dbs = db.plugin_registry(RegistryConfig::default());
    let wiring = wiring::build(&db, &config.plugin_settings).await?;
    for note in &wiring.notes {
        logger.info(MODULE, note);
    }
    let wiring::Wiring {
        plugins,
        sinks,
        notes: _,
        phigros_webui,
    } = wiring;
    for plugin in plugins {
        let plugin_id = plugin.id.clone();
        let assets = Arc::new(PluginAssets::new(
            plugin_id.clone(),
            Path::new(ASSETS_ROOT).join(&plugin_id),
        ));
        let report = assets.bootstrap(&templates);
        logger.info(
            MODULE,
            &format!(
                "插件 {plugin_id} 素材: 字体 {} 个, 模板 {} 个, 跳过 {} 个",
                report.fonts.len(),
                report.templates.len(),
                report.skipped.len()
            ),
        );
        let loader = assets.loader();
        book.insert(plugin_id.clone(), assets);
        loaders.push((plugin_id, loader));
        let commands = registry.register_plugin(plugin);
        logger.info(MODULE, &format!("已挂载插件指令: {commands:?}"));
    }
    logger.info(
        MODULE,
        &format!(
            "共注册 {} 条指令, {} 个插件带素材目录",
            registry.command_count(),
            book.len()
        ),
    );

    let render_workers = config.render_pool.max(1);
    let scheduler_config = SchedulerConfig {
        workers: render_workers,
        ..SchedulerConfig::default()
    };
    let backend = SkiaBackend::handle();
    let scheduler = RenderScheduler::try_new(scheduler_config, Arc::clone(&backend))?;
    let texture_capacity = scheduler_config.texture_capacity().max(256 * 1024 * 1024);
    let merged_loader: Arc<dyn aurorix_render::AssetLoader> = Arc::new(
        aurorix_render::ChainedAssets::new(loaders.iter().map(|(_, loader)| Arc::clone(loader)).collect()),
    );
    let fallback_context =
        RenderContext::new(Arc::clone(&backend), PLUGIN_FONT_CAPACITY, texture_capacity)
            .with_asset_loader(merged_loader);
    let contexts: BTreeMap<String, Arc<RenderContext>> = loaders
        .into_iter()
        .map(|(plugin_id, loader)| {
            let context =
                RenderContext::new(Arc::clone(&backend), PLUGIN_FONT_CAPACITY, texture_capacity)
                    .with_asset_loader(loader);
            (plugin_id, context)
        })
        .collect();
    logger.info(
        MODULE,
        &format!("插件渲染上下文: {} 个已挂载素材加载器", contexts.len()),
    );
    logger.info(
        MODULE,
        &format!(
            "渲染调度器已就绪 workers={render_workers} 硬件加速={}",
            if aurorix_render::gpu::hardware_accelerated() {
                "已启用"
            } else {
                "未启用"
            }
        ),
    );

    let api = Arc::new(BotApi::new(api_config(&config)));
    api.set_upload_threshold(config.upload_threshold);
    api.set_retry_when(&config.retry_when);
    api.set_image_box(config.image_box());
    install_official_upload(Arc::clone(&api));
    let host = Arc::new(aurorix_bot::build_host(db.as_ref()).await);
    let images = Arc::new(HostImageStore::new(Arc::clone(&host)));
    logger.info(MODULE, &format!("图床链路: {:?}", host.providers()));

    let jobs = Arc::new(JobRegistry::new());
    let inbox = Arc::new(db.inbox());
    spawn_name_backfill(
        Arc::clone(&api),
        Arc::clone(&inbox),
        Arc::new(db.stats()),
        Arc::clone(&logger),
    );

    if let Some(webui) = phigros_webui {
        aurorix_admin::set_plugin_web_uis(vec![
            aurorix_admin::PluginWebUi::new("phigros")
                .with_api(phigros_webui_router(webui))
                .with_spa_root(Path::new(ASSETS_ROOT).join("phigros").join("web")),
        ]);
        logger.info(MODULE, "phigros 管理页已挂载: /phigros/");
    }
    let panel_sync = config.panel_sync;
    let renderer = Arc::new(
        SchedulerRenderer::new(Arc::clone(&scheduler))
            .with_contexts(contexts)
            .with_fallback_context(fallback_context),
    ) as Arc<dyn Renderer>;
    let mut builder = Bot::builder(config)
        .with_api(Arc::clone(&api))
        .with_tokens(Arc::new(ApiTokens::new(Arc::clone(&api))))
        .with_kv(Arc::new(db.kv_global()) as Arc<dyn KvBackend>)
        .with_inbox(Arc::clone(&inbox) as Arc<dyn InboxArchive>)
        .with_stats(Arc::new(DbStats::new(Arc::new(db.stats()))))
        .with_render(renderer)
        .with_images(Arc::clone(&images) as Arc<dyn aurorix_api::ImageStore>)
        .with_asset_reload(Arc::new(HostReload::new(
            Arc::clone(&images),
            Arc::clone(&db),
        )))
        .with_plugin_dbs(Arc::new(plugin_dbs))
        .with_plugin_assets(Arc::new(book))
        .with_templates(templates)
        .with_template_override(TEMPLATE_OVERRIDE_DIR)
        .with_database(Arc::clone(&db))
        .with_plugins(registry)
        .with_config_path(config_path())
        .with_app_config(app_config);
    for sink in sinks {
        builder = builder.with_plugin_config(sink);
    }
    if panel_sync {
        builder = builder.with_panel_remote(Arc::new(ApiPanelRemote::new(Arc::clone(&api))));
    }
    let bot = builder
        .with_logger(Arc::clone(&logger))
        .with_jobs(Arc::clone(&jobs))
        .build()?;

    logger.info(MODULE, "Aurorix 启动中");
    let report = bot.serve().await?;
    scheduler.shutdown().await;
    logger.info(MODULE, "渲染调度器已退出");
    logger.info(MODULE, &format!("已退出: {report:?}"));
    Ok(())
}

fn spawn_name_backfill(
    api: Arc<BotApi>,
    inbox: Arc<InboxStore>,
    stats: Arc<aurorix_db::StatsStore>,
    logger: Arc<Logger>,
) {
    tokio::spawn(async move {
        let conversations = match inbox.conversations().await {
            Ok(list) => list,
            Err(error) => {
                logger.warn(MODULE, &format!("读取会话列表失败, 群名回填跳过: {error}"));
                return;
            }
        };
        let mut filled = 0usize;
        for conv in conversations {
            if conv.kind != Scene::Group || !conv.name.trim().is_empty() {
                continue;
            }
            if let Ok(info) = api.group_info(&conv.id).await {
                if !info.name().is_empty() && inbox.set_name(&conv.id, info.name()).await.is_ok() {
                    let _ = stats.set_name(Scene::Group, &conv.id, info.name()).await;
                    filled += 1;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        if filled > 0 {
            logger.info(MODULE, &format!("回填群名 {filled} 个"));
        }
    });
}

fn install_official_upload(api: Arc<BotApi>) {
    let upload: OfficialUploadFn = Arc::new(move |request: OfficialUploadRequest| {
        let api = Arc::clone(&api);
        Box::pin(async move {
            api.upload_official_image(
                &request.target,
                request.bytes,
                &request.filename,
                &request.mime,
            )
            .await
            .map_err(|error| AssetError::Message(error.to_string()))
        })
    });
    set_official_upload(Some(upload));
}

fn config_path() -> PathBuf {
    env::var(CONFIG_PATH_ENV)
        .unwrap_or_else(|_| CONFIG_FILE.to_string())
        .into()
}

fn load_config() -> Result<(AppConfig, BotConfig), Box<dyn Error>> {
    let path = config_path();
    let mut app = if Path::new(&path).is_file() {
        AppConfig::load(&path)?
    } else {
        let mut fallback = AppConfig::default();
        fallback.normalize();
        fallback
    };
    app.appid = env_var("AURORIX_APPID", app.appid);
    app.secret = env_var("AURORIX_SECRET", app.secret);
    app.database = env_var("AURORIX_DATABASE", app.database);
    app.admin_password = env_var("AURORIX_ADMIN_PASSWORD", app.admin_password);
    if let Some(protocol) = env::var("AURORIX_PROTOCOL")
        .ok()
        .filter(|value| !value.is_empty())
    {
        app.protocol = Some(protocol);
    }
    if let Some(port) = env::var("AURORIX_PORT")
        .ok()
        .and_then(|raw| raw.parse().ok())
    {
        app.port = port;
    }
    let config = BotConfig::from_app_config(&app);
    config.validate()?;
    Ok((app, config))
}

fn api_config(config: &BotConfig) -> aurorix_api::BotConfig {
    let mut api = aurorix_api::BotConfig::new(config.appid.clone(), config.secret.clone());
    if !config.proxy_api.trim().is_empty() {
        api.base_url = config.proxy_api.clone();
    }
    api
}

fn env_var(key: &str, fallback: String) -> String {
    env::var(key).unwrap_or(fallback)
}

fn phigros_webui_router(webui: aurorix_phigros_plugin::webui::WebuiService) -> axum::Router {
    use aurorix_phigros_plugin::webui::{Method, WebuiRequest};
    let webui = Arc::new(webui);
    axum::Router::new().route(
        "/{*rest}",
        axum::routing::any(move |request: axum::extract::Request| {
        let webui = Arc::clone(&webui);
        async move {
            let (parts, body) = request.into_parts();
            let bytes = axum::body::to_bytes(body, 1 << 20)
                .await
                .unwrap_or_default();
            let method = match parts.method.as_str() {
                "POST" => Method::Post,
                "PUT" => Method::Put,
                "DELETE" => Method::Delete,
                _ => Method::Get,
            };
            let raw = parts.uri.path();
            let relative = raw.strip_prefix("/phigros/api").unwrap_or(raw);
            let query = parts
                .uri
                .query()
                .map(|value| format!("?{value}"))
                .unwrap_or_default();
            let request = WebuiRequest {
                method,
                path: format!("/api{relative}{query}"),
                body: String::from_utf8_lossy(&bytes).to_string(),
            };
            let response = webui.route_async(&request).await;
            let status = axum::http::StatusCode::from_u16(response.status).unwrap_or_default();
            let mut output =
                axum::response::Response::new(axum::body::Body::from(response.json));
            *output.status_mut() = status;
            output.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json; charset=utf-8"),
            );
            output
        }
    }))
}
