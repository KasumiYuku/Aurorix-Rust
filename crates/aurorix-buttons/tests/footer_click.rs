use std::sync::Arc;

use aurorix_buttons::{
    footer_count_key, CallbackRegistry, FooterAction, FooterBucket, FooterClickError, FooterDedup,
    FooterHub, FooterScope, FOOTER_DEFAULT_REPLY,
};

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn hub_with(scope: FooterScope, items: &[&str]) -> FooterHub {
    let hub = FooterHub::new();
    hub.set_lines(scope, lines(items));
    hub
}

#[test]
fn counter_falls_back_to_user_group_and_default_reply() {
    let hub = hub_with(FooterScope::Global, &["+1 | counter"]);
    let (result, errors) = hub.current_result(FooterScope::Global);
    assert!(errors.is_empty(), "全局配置行应当全部解析成功: {errors:?}");
    let data = FooterScope::Global.slot_base().to_string();
    let click = hub
        .resolve_click(&data)
        .expect("全局第一个底部按钮应当能解析出动作");
    assert_eq!(
        click.scope,
        FooterScope::Global,
        "槽位 0 应当落在全局作用域"
    );
    assert_eq!(click.index, 0, "槽位 0 的组内下标应当是 0");
    assert_eq!(click.slot, 0, "解析结果应当回填原始槽位号");
    assert_eq!(click.label, "+1", "点击结果的文案应当就是配置里的标签");
    match click.action {
        FooterAction::Counter {
            count_key,
            dedup,
            bucket,
            reply,
            duplicate_reply,
            label,
        } => {
            assert_eq!(
                count_key,
                footer_count_key(FooterScope::Global, &result.specs[0]),
                "计数键应当与 footer_count_key 算出来的完全一致"
            );
            assert_eq!(count_key, "footer.+1", "全局作用域的计数键不带作用域前缀");
            assert_eq!(dedup, FooterDedup::User, "没写 dedup 时默认按用户去重");
            assert_eq!(bucket, FooterBucket::Group, "没写 scope 时默认记进群聊桶");
            assert_eq!(
                reply, FOOTER_DEFAULT_REPLY,
                "回复模板为空时应当回落成默认文案"
            );
            assert!(duplicate_reply.is_none(), "没写 dup 时不应有重复点击文案");
            assert_eq!(label, "+1", "计数动作里也要带上按钮文案");
        }
        other => panic!("计数按钮应当解析成 Counter, 实际是 {other:?}"),
    }
}

#[test]
fn counter_reads_dedup_scope_and_dup_options() {
    let hub = hub_with(
        FooterScope::Global,
        &[
            "赞 | counter | ; dedup=none ; scope=global ; dup={{count}}",
            "踩 | counter | 已经记下 {{count}} 次了 ; dedup=group",
        ],
    );
    let (result, errors) = hub.current_result(FooterScope::Global);
    assert!(errors.is_empty(), "两行计数配置都应当解析成功: {errors:?}");

    let first = hub
        .resolve_click(&FooterScope::Global.slot_base().to_string())
        .expect("第一个计数按钮应当能解析出动作");
    match first.action {
        FooterAction::Counter {
            dedup,
            bucket,
            reply,
            duplicate_reply,
            ..
        } => {
            assert_eq!(dedup, FooterDedup::None, "dedup=none 应当解析成不去重");
            assert_eq!(bucket, FooterBucket::Global, "scope=global 应当落到全局桶");
            assert_eq!(
                reply, FOOTER_DEFAULT_REPLY,
                "参数段只有选项时回复模板为空, 应当回落成默认文案"
            );
            assert_eq!(
                duplicate_reply,
                Some("{{count}}".to_string()),
                "dup={{count}} 应当原样读成重复点击文案"
            );
        }
        other => panic!("dedup=none 的按钮应当解析成 Counter, 实际是 {other:?}"),
    }

    let second = hub
        .resolve_click(&(FooterScope::Global.slot_base() + 1).to_string())
        .expect("第二个计数按钮应当能解析出动作");
    assert_eq!(second.label, "踩", "第二个按钮取到的应当是第二行的标签");
    match second.action {
        FooterAction::Counter {
            dedup,
            bucket,
            reply,
            duplicate_reply,
            ..
        } => {
            assert_eq!(dedup, FooterDedup::Group, "dedup=group 应当解析成按群去重");
            assert_eq!(bucket, FooterBucket::Group, "没写 scope 时桶保持群聊");
            assert_eq!(
                reply, "已经记下 {{count}} 次了",
                "参数段有内容时回复模板应当原样保留"
            );
            assert!(duplicate_reply.is_none(), "没写 dup 时不应有重复点击文案");
        }
        other => panic!("dedup=group 的按钮应当解析成 Counter, 实际是 {other:?}"),
    }
    assert_eq!(result.specs.len(), 2, "两行配置应当得到两个底部按钮");
}

#[test]
fn non_counter_kinds_map_to_their_own_actions() {
    CallbackRegistry::global().register("test.footer.click", Arc::new(()));
    let hub = hub_with(
        FooterScope::Global,
        &[
            "帮助 | command | /help",
            "文档 | link | https://example.com/docs",
            "复读 | repeat",
            "图片 | image | https://example.com/p.png",
            "指南 | callback | test.footer.click",
        ],
    );
    let (_, errors) = hub.current_result(FooterScope::Global);
    assert!(
        errors.is_empty(),
        "五种非计数按钮都应当解析成功: {errors:?}"
    );
    let base = FooterScope::Global.slot_base();

    let help = hub
        .resolve_click(&base.to_string())
        .expect("指令按钮应当能解析出动作");
    assert_eq!(
        help.action,
        FooterAction::Command,
        "指令按钮应当落到 Command"
    );
    assert_eq!(help.label, "帮助", "指令按钮的文案应当来自配置");

    let docs = hub
        .resolve_click(&(base + 1).to_string())
        .expect("链接按钮应当能解析出动作");
    assert_eq!(docs.action, FooterAction::Link, "链接按钮应当落到 Link");

    let repeat = hub
        .resolve_click(&(base + 2).to_string())
        .expect("复读按钮应当能解析出动作");
    assert_eq!(
        repeat.action,
        FooterAction::Repeat,
        "复读按钮应当落到 Repeat"
    );

    let image = hub
        .resolve_click(&(base + 3).to_string())
        .expect("图片按钮应当能解析出动作");
    match image.action {
        FooterAction::Image { url, label } => {
            assert_eq!(
                url, "https://example.com/p.png",
                "图片动作应当带上配置里的图片链接"
            );
            assert_eq!(label, "图片", "图片动作应当带上按钮文案");
        }
        other => panic!("图片按钮应当解析成 Image, 实际是 {other:?}"),
    }

    let callback = hub
        .resolve_click(&(base + 4).to_string())
        .expect("回调按钮应当能解析出动作");
    assert_eq!(
        callback.action,
        FooterAction::Callback,
        "回调按钮应当落到 Callback"
    );
}

#[test]
fn rejects_bad_data_unknown_slot_and_stale_config() {
    let hub = hub_with(FooterScope::Global, &["+1 | counter"]);

    match hub.resolve_click("abc") {
        Err(FooterClickError::BadData(message)) => {
            assert!(
                message.contains("底部按钮回调数据非法"),
                "非数字回调数据应当提示数据非法, 实际是 {message:?}"
            );
            assert!(
                message.contains("abc"),
                "错误信息里应当带上原始回调数据, 实际是 {message:?}"
            );
        }
        other => panic!("非数字回调数据应当报 BadData, 实际是 {other:?}"),
    }
    match hub.resolve_click("   ") {
        Err(FooterClickError::BadData(message)) => {
            assert!(
                message.contains("底部按钮回调数据非法"),
                "只有空白的回调数据应当按非法数据处理, 实际是 {message:?}"
            );
        }
        other => panic!("空白回调数据应当报 BadData, 实际是 {other:?}"),
    }
    match hub.resolve_click("99") {
        Err(FooterClickError::UnknownSlot(slot)) => {
            assert_eq!(slot, 99, "越界槽位应当原样带回错误里");
            assert_eq!(
                FooterClickError::UnknownSlot(99).to_string(),
                "底部按钮槽位 99 不属于任何作用域",
                "越界槽位的提示文案应当符合约定"
            );
        }
        other => panic!("槽位 99 应当报 UnknownSlot, 实际是 {other:?}"),
    }
    match hub.resolve_click("-1") {
        Err(FooterClickError::UnknownSlot(slot)) => {
            assert_eq!(slot, -1, "负数槽位也应当按越界处理");
        }
        other => panic!("负数槽位应当报 UnknownSlot, 实际是 {other:?}"),
    }

    let stale = hub
        .resolve_click(&(FooterScope::Global.slot_base() + 3).to_string())
        .expect_err("配置里只有 1 个按钮时, 下标 3 应当判定为配置过期");
    match stale {
        FooterClickError::StaleConfig { scope, index } => {
            assert_eq!(scope, FooterScope::Global, "配置过期应当指向全局作用域");
            assert_eq!(index, 3, "配置过期应当带上越界的组内下标");
        }
        other => panic!("下标越界应当报 StaleConfig, 实际是 {other:?}"),
    }
    assert_eq!(
        FooterClickError::StaleConfig {
            scope: FooterScope::SceneGroup,
            index: 3,
        }
        .to_string(),
        "群聊底部按钮 #3 已不在配置里(配置刚改过?), 忽略本次点击",
        "配置过期的提示文案应当带上作用域中文名和序号"
    );
}

#[test]
fn scopes_do_not_leak_into_each_other() {
    let hub = FooterHub::new();
    hub.set_lines(
        FooterScope::SceneGroup,
        lines(&["点赞 | counter", "帮助 | command | /help"]),
    );
    hub.set_lines(FooterScope::SceneC2c, lines(&["赞 | counter"]));

    let group_first = hub
        .resolve_click(&FooterScope::SceneGroup.slot_base().to_string())
        .expect("群聊第一个按钮应当能解析出动作");
    assert_eq!(
        group_first.scope,
        FooterScope::SceneGroup,
        "群聊槽位应当解析回群聊作用域"
    );
    assert_eq!(group_first.index, 0, "群聊第一个按钮的组内下标是 0");
    match group_first.action {
        FooterAction::Counter { count_key, .. } => {
            assert_eq!(
                count_key, "footer.scene.group.点赞",
                "群聊计数键应当带 scene.group 前缀"
            );
        }
        other => panic!("群聊第一个按钮应当解析成 Counter, 实际是 {other:?}"),
    }

    let group_second = hub
        .resolve_click(&(FooterScope::SceneGroup.slot_base() + 1).to_string())
        .expect("群聊第二个按钮应当能解析出动作");
    assert_eq!(
        group_second.scope,
        FooterScope::SceneGroup,
        "群聊第二个槽位仍属于群聊作用域"
    );
    assert_eq!(group_second.index, 1, "群聊第二个按钮的组内下标是 1");
    assert_eq!(
        group_second.action,
        FooterAction::Command,
        "群聊第二个按钮是指令按钮, 应当落到 Command"
    );

    let c2c_first = hub
        .resolve_click(&FooterScope::SceneC2c.slot_base().to_string())
        .expect("单聊第一个按钮应当能解析出动作");
    assert_eq!(
        c2c_first.scope,
        FooterScope::SceneC2c,
        "单聊槽位不能被解析成群聊作用域"
    );
    assert_eq!(c2c_first.index, 0, "单聊第一个按钮的组内下标是 0");
    assert_eq!(c2c_first.label, "赞", "单聊按钮应当取单聊的配置");
    match c2c_first.action {
        FooterAction::Counter { count_key, .. } => {
            assert_eq!(
                count_key, "footer.scene.c2c.赞",
                "单聊计数键应当带 scene.c2c 前缀"
            );
        }
        other => panic!("单聊第一个按钮应当解析成 Counter, 实际是 {other:?}"),
    }
}

#[test]
fn shrunk_config_turns_old_index_into_stale() {
    let hub = FooterHub::new();
    hub.set_lines(
        FooterScope::SceneGroup,
        lines(&["一 | counter", "二 | counter"]),
    );
    let data = (FooterScope::SceneGroup.slot_base() + 1).to_string();
    let click = hub
        .resolve_click(&data)
        .expect("配置有 2 个按钮时下标 1 应当可用");
    assert_eq!(click.index, 1, "热更前解析出的组内下标应当是 1");
    assert_eq!(click.label, "二", "热更前下标 1 取到的应当是第二个按钮");

    hub.set_lines(FooterScope::SceneGroup, lines(&["一 | counter"]));
    let stale = hub
        .resolve_click(&data)
        .expect_err("配置只剩 1 个按钮后, 同一个回调应当判定为配置过期");
    match stale {
        FooterClickError::StaleConfig { scope, index } => {
            assert_eq!(scope, FooterScope::SceneGroup, "过期判定应当指向群聊作用域");
            assert_eq!(index, 1, "过期判定应当保留原来的组内下标 1");
        }
        other => panic!("配置缩水后应当报 StaleConfig, 实际是 {other:?}"),
    }

    let survive = hub
        .resolve_click(&FooterScope::SceneGroup.slot_base().to_string())
        .expect("配置缩水后下标 0 仍然有效");
    assert_eq!(survive.label, "一", "配置缩水后下标 0 仍应指向第一个按钮");
}
