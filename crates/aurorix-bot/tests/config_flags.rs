use std::path::PathBuf;

use aurorix_bot::BotConfig;
use aurorix_db::AppConfig;

fn example_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config.example.json")
        .canonicalize()
        .expect("仓库根目录应当存在 config.example.json")
}

#[test]
fn upload_threshold_and_retry_when_reach_the_bot_config() {
    let mut app = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        upload_threshold: Some(1024 * 1024),
        retry_when: Some(vec![40034025, 304023]),
        ..AppConfig::default()
    };
    app.normalize();
    let config = BotConfig::from_app_config(&app);
    assert_eq!(config.upload_threshold, 1024 * 1024, "阈值应当来自配置文件");
    assert_eq!(
        config.retry_when,
        vec![40034025, 304023],
        "重试码应当来自配置文件"
    );
}

#[test]
fn defaults_apply_when_the_fields_are_absent() {
    let mut app = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        ..AppConfig::default()
    };
    app.normalize();
    let config = BotConfig::from_app_config(&app);
    assert_eq!(
        config.upload_threshold,
        aurorix_core::DEFAULT_UPLOAD_THRESHOLD,
        "缺省阈值应当是 3 MiB"
    );
    assert_eq!(
        config.retry_when,
        aurorix_protocol::DEFAULT_RETRY_WHEN.to_vec(),
        "缺省重试码应当是平台约定的两个业务码"
    );
}

#[test]
fn image_limits_reach_the_api_box() {
    let mut app = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        image_max_width: Some(512),
        image_max_height: Some(300),
        ..AppConfig::default()
    };
    app.normalize();
    let config = BotConfig::from_app_config(&app);
    let image_box = config.image_box();
    assert_eq!(image_box.width, 512);
    assert_eq!(image_box.height, 300);
    assert!(image_box.on());
}

#[test]
fn omitted_image_limits_fall_back_to_the_go_defaults() {
    let mut app = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        ..AppConfig::default()
    };
    app.normalize();
    let config = BotConfig::from_app_config(&app);
    let image_box = config.image_box();
    assert_eq!(image_box.width, 350);
    assert_eq!(image_box.height, 438);
    assert_eq!(config.image_limits_enabled, None, "省略即开启");
}

#[test]
fn image_limits_switch_off_disables_the_box() {
    let mut app = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        image_limits_enabled: Some(false),
        ..AppConfig::default()
    };
    app.normalize();
    let config = BotConfig::from_app_config(&app);
    assert!(!config.image_box().on(), "关掉开关后不再夹取");
    assert_eq!(config.image_box().limit(1000, 2000), (1000, 2000));
}

#[test]
fn out_of_range_image_sides_fall_back_to_the_defaults() {
    let mut app = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        image_max_width: Some(9000),
        image_max_height: Some(10),
        ..AppConfig::default()
    };
    app.normalize();
    let image_box = BotConfig::from_app_config(&app).image_box();
    assert_eq!(
        (image_box.width, image_box.height),
        (350, 10),
        "超出 4096 的退回默认, 与 Go 的 normalize 一致"
    );
}

#[test]
fn shipped_example_config_produces_a_single_listen_address() {
    let app = AppConfig::load(example_config_path()).expect("示例配置应当可以解析");
    let config = BotConfig::from_app_config(&app);
    config.validate().expect("示例配置应当通过校验");
    let addr = config.admin_addr();
    assert_eq!(
        addr.matches(':').count(),
        1,
        "监听地址只应包含一个端口分隔符, 实际为 {addr}"
    );
    assert_eq!(addr, "127.0.0.1:8080", "示例应当监听 127.0.0.1:8080");
}

#[test]
fn global_markdown_flag_reaches_the_bot_config() {
    let mut app = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        global_markdown: Some(true),
        ..AppConfig::default()
    };
    app.normalize();
    let config = BotConfig::from_app_config(&app);
    assert!(
        config.global_markdown,
        "global_markdown: true 要传到机器人配置"
    );
    let mut plain = AppConfig {
        appid: String::from("app"),
        secret: String::from("secret"),
        ..AppConfig::default()
    };
    plain.normalize();
    assert!(
        !BotConfig::from_app_config(&plain).global_markdown,
        "没写这一项时默认关闭, 与 Go 原版示例一致"
    );
}
