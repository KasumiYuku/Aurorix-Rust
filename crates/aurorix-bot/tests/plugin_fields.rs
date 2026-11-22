use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use aurorix_admin::{PluginError, PluginSource};
use aurorix_bot::bot::AdminPlugins;
use aurorix_bot::{ClosurePluginConfig, PluginConfigStore};
use aurorix_plugin::{CommandRegistry, CommandSpec, PluginCommands, PluginConfigField};
use serde_json::{json, Map, Value};

fn registry() -> Arc<CommandRegistry> {
    let registry = Arc::new(CommandRegistry::new());
    let plugin = PluginCommands::new("arcaea", "猜歌")
        .command(CommandSpec::new("rating"))
        .config(
            PluginConfigField::text("api_key", "接口密钥")
                .description("开放平台的接口密钥")
                .required(),
        )
        .config(PluginConfigField::number("limit", "次数上限").default_value(10))
        .config(PluginConfigField::boolean("enabled", "启用").default_value(true))
        .config(PluginConfigField::select("mode", "模式", ["宽松", "严格"]).default_value("严格"));
    registry.register_plugin(plugin);
    registry
}

struct Harness {
    plugins: AdminPlugins,
    applied: Arc<Mutex<Vec<BTreeMap<String, Value>>>>,
}

fn harness(failure: Option<&str>) -> Harness {
    let store = Arc::new(PluginConfigStore::new(None, BTreeMap::new()));
    let applied = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&applied);
    let reason = failure.map(str::to_string);
    store.attach(Arc::new(ClosurePluginConfig::new(
        "arcaea",
        move |settings| {
            if let Some(reason) = &reason {
                return Err(reason.clone());
            }
            seen.lock().expect("锁不该被毒化").push(settings.clone());
            Ok(())
        },
    )));
    Harness {
        plugins: AdminPlugins::new(registry(), store),
        applied,
    }
}

fn payload(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

#[test]
fn drawer_sees_declared_fields_and_defaults() {
    let harness = harness(None);
    let plugins = harness.plugins.plugins();
    assert_eq!(plugins.len(), 1, "只注册了一个插件");
    let plugin = &plugins[0];
    assert_eq!(plugin.id, "arcaea");
    assert_eq!(plugin.commands, vec![String::from("rating")]);
    assert_eq!(plugin.fields.len(), 4, "四个声明项都要出现在抽屉里");
    assert_eq!(plugin.fields[0]["key"], json!("api_key"));
    assert_eq!(plugin.fields[0]["type"], json!("text"));
    assert_eq!(plugin.fields[0]["required"], json!(true));
    assert_eq!(plugin.fields[1]["type"], json!("number"));
    assert_eq!(plugin.fields[2]["type"], json!("boolean"));
    assert_eq!(plugin.fields[3]["type"], json!("select"));
    assert_eq!(plugin.fields[3]["options"], json!(["宽松", "严格"]));
    assert_eq!(plugin.values["limit"], json!(10), "没保存过时展示默认值");
    assert_eq!(plugin.values["mode"], json!("严格"));
}

#[test]
fn saving_applies_and_reports_new_values() {
    let harness = harness(None);
    harness
        .plugins
        .save_config("arcaea", payload(&[("limit", json!(30))]))
        .expect("保存要成功");
    let applied = harness.applied.lock().expect("锁不该被毒化");
    assert_eq!(applied.len(), 1, "热更新要立刻作用到插件");
    assert_eq!(applied[0]["limit"], json!(30));
    assert_eq!(applied[0]["mode"], json!("严格"), "未提交的项保留默认值");
    drop(applied);
    let plugin = harness.plugins.plugin("arcaea").expect("插件要能取到");
    assert_eq!(plugin.values["limit"], json!(30), "抽屉要看到新的值");
}

#[test]
fn string_forms_are_normalized_before_applying() {
    let harness = harness(None);
    harness
        .plugins
        .save_config(
            "arcaea",
            payload(&[("limit", json!("45")), ("enabled", json!("off"))]),
        )
        .expect("字符串形式要能保存");
    let applied = harness.applied.lock().expect("锁不该被毒化");
    assert_eq!(applied[0]["limit"], json!(45));
    assert_eq!(applied[0]["enabled"], json!(false));
}

#[test]
fn unknown_key_is_rejected_before_applying() {
    let harness = harness(None);
    let error = harness
        .plugins
        .save_config("arcaea", payload(&[("nope", json!(1))]))
        .expect_err("未声明的配置项要拒绝");
    match error {
        PluginError::BadRequest(message) => {
            assert!(message.contains("nope"), "错误要点出键名: {message}");
        }
        other => panic!("应当是参数错误, 实际是 {other}"),
    }
    assert!(
        harness.applied.lock().expect("锁不该被毒化").is_empty(),
        "被拒绝时不能热更新"
    );
}

#[test]
fn wrong_type_is_rejected() {
    let harness = harness(None);
    let error = harness
        .plugins
        .save_config("arcaea", payload(&[("limit", json!("很多"))]))
        .expect_err("类型不符要拒绝");
    match error {
        PluginError::BadRequest(message) => {
            assert!(
                message.contains("次数上限"),
                "错误要指出是哪一项: {message}"
            );
        }
        other => panic!("应当是参数错误, 实际是 {other}"),
    }
}

#[test]
fn sink_failure_surfaces_as_server_error() {
    let harness = harness(Some("令牌无效"));
    let error = harness
        .plugins
        .save_config("arcaea", payload(&[("api_key", json!("abc"))]))
        .expect_err("插件拒绝时要报错");
    match error {
        PluginError::Server(message) => {
            assert!(
                message.contains("令牌无效"),
                "错误要带上插件原因: {message}"
            );
        }
        other => panic!("应当是服务端错误, 实际是 {other}"),
    }
}

#[test]
fn unknown_plugin_returns_not_found() {
    let harness = harness(None);
    let error = harness
        .plugins
        .save_config("nobody", payload(&[]))
        .expect_err("不存在的插件要报错");
    match error {
        PluginError::NotFound(message) => {
            assert!(message.contains("nobody"), "错误要点出插件 id: {message}");
        }
        other => panic!("应当是未找到, 实际是 {other}"),
    }
}
