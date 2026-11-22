use aurorix_buttons::{
    footer_button_id, footer_buttons, footer_slot_of, footer_style, parse_footer, ButtonAction,
    ButtonStyle, FooterHub, FooterKind, FooterScope, Keyboard,
};
use aurorix_core::Scene;

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

#[test]
fn parses_valid_lines_with_chinese_aliases() {
    let result = parse_footer(&lines(&[
        "# 注释行与空行应当跳过",
        "",
        "  +1   | counter | 感谢支持，当前 {{count}} 次 ; dedup=user",
        "帮助 | 指令 | /help",
        "文档 | 链接 | https://example.com/docs",
    ]));
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.specs.len(), 3);
    let counter = &result.specs[0];
    assert_eq!(counter.kind(), FooterKind::Counter);
    assert_eq!(counter.label, "+1");
    assert_eq!(counter.key, "+1");
    assert_eq!(counter.option("dedup", ""), "user");
    assert_eq!(result.specs[1].kind(), FooterKind::Command);
    assert_eq!(result.specs[1].arg, "/help");
    assert_eq!(result.specs[2].kind(), FooterKind::Link);
}

#[test]
fn rejects_bad_lines_with_line_numbers() {
    let cases: [(&str, &str, &str); 14] = [
        ("段数不足", "+1", "至少要写"),
        ("标签为空", "  | counter", "标签不能为空"),
        ("类型不认识", "+1 | nope", "类型 \"nope\" 不认识"),
        (
            "link 没有链接",
            "文档 | link",
            "link 的第 3 段要是 http(s) 链接",
        ),
        (
            "link 协议不对",
            "文档 | link | ftp://example.com",
            "link 的第 3 段要是 http(s) 链接",
        ),
        (
            "command 没有指令",
            "帮助 | command",
            "command 要在第 3 段写指令内容",
        ),
        (
            "选项不认识",
            "+1 | counter |  | nope=1",
            "不支持选项 \"nope\"",
        ),
        ("选项不是 k=v", "+1 | counter |  | dedup", "不是 k=v 形式"),
        (
            "style 取值错",
            "+1 | counter |  | style=red",
            "style 只能是 blue / gray / 0~4",
        ),
        (
            "dedup 取值错",
            "+1 | counter |  | dedup=everyone",
            "dedup 只能是 user / group / none",
        ),
        (
            "scope 取值错",
            "+1 | counter |  | scope=all",
            "scope 只能是 group 或 global",
        ),
        (
            "auto 取值错",
            "帮助 | command | /help | auto=yes",
            "auto 只能是 0 或 1",
        ),
        ("段数过多", "+1 | counter | a | b | c", "最多 4 段"),
        ("占位符写错", "+1 | counter | 当前 {{nope}} 次", "占位符"),
    ];
    for (name, line, expected) in cases {
        let result = parse_footer(&lines(&[line]));
        assert_eq!(result.errors.len(), 1, "{name}: {:?}", result.errors);
        let message = &result.errors[0];
        assert!(
            message.contains(expected),
            "{name}: {message} 缺少 {expected}"
        );
        assert!(
            message.contains("第 1 行"),
            "{name}: 错误应带行号 {message}"
        );
        assert!(result.specs.is_empty(), "{name}: 非法行不该产出按钮");
    }
}

#[test]
fn keeps_valid_lines_and_reports_bad_line_number() {
    let result = parse_footer(&lines(&[
        "+1 | counter | 好的",
        "坏的 | 类型",
        "帮助 | command | /help",
    ]));
    assert_eq!(result.specs.len(), 2);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("第 2 行"), "{:?}", result.errors);
}

#[test]
fn rejects_duplicate_counter_keys() {
    let result = parse_footer(&lines(&["点赞 | counter | 好的", "点赞 | counter | 再来"]));
    assert_eq!(result.specs.len(), 1);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("计数键"));
}

#[test]
fn caps_button_count_at_five() {
    let mut list = Vec::new();
    for _ in 0..6 {
        list.push("帮助 | command | /help".to_string());
    }
    let result = parse_footer(&list);
    assert_eq!(result.specs.len(), 5);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("最多"));
}

#[test]
fn splits_multiple_buttons_per_line_without_breaking_text_commas() {
    let result = parse_footer(&lines(&[
        "+1 | counter | 感谢支持, 当前 {{count}} 次 , 帮助 | command | /help",
        "文档 | link | https://example.com",
    ]));
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.specs.len(), 3);
    assert_eq!(result.specs[0].arg, "感谢支持, 当前 {{count}} 次");
    assert_eq!(result.specs[1].kind(), FooterKind::Command);
    assert_eq!(result.specs[1].arg, "/help");
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0].len(), 2);
    assert_eq!(result.rows[1].len(), 1);
}

#[test]
fn builds_buttons_with_go_ids_and_actions() {
    let specs = parse_footer(&lines(&[
        "+1 | counter | 好的",
        "帮助 | command | /help | auto=1",
        "文档 | link | https://example.com",
    ]))
    .specs;
    let row = footer_buttons(&specs, "");
    assert_eq!(row.len(), 3);
    assert_eq!(row[0].id, "aurorix.footer.0");
    assert_eq!(row[2].id, "aurorix.footer.2");
    assert_eq!(row[0].action.action_type, ButtonAction::Callback);
    assert_eq!(row[0].action.callback_data.as_deref(), Some("0"));
    assert_eq!(row[1].action.action_type, ButtonAction::Command);
    assert!(row[1].action.auto_send);
    assert_eq!(row[1].action.msg.as_deref(), Some("/help"));
    assert_eq!(row[2].action.action_type, ButtonAction::Link);
    assert_eq!(row[2].action.url.as_deref(), Some("https://example.com"));

    let mut keyboard = Keyboard::new();
    assert!(keyboard.append_row(row));
    let raw = String::from_utf8(keyboard.to_json().expect("json")).expect("utf8");
    for expected in [
        "\"type\":1",
        "\"data\":\"0\"",
        "\"type\":2",
        "\"enter\":true",
        "\"type\":0",
    ] {
        assert!(
            raw.contains(expected),
            "键盘 JSON 里找不到 {expected}: {raw}"
        );
    }
}

#[test]
fn footer_permissions_allow_everyone_by_default() {
    let specs = parse_footer(&lines(&["+1 | counter | 好的"])).specs;
    for button in footer_buttons(&specs, "") {
        assert_eq!(
            button.action.permission.permission_type,
            aurorix_buttons::AllowedPermission::AllUser
        );
    }
}

fn hub_with(lines_set: &[&str]) -> FooterHub {
    let hub = FooterHub::new();
    hub.set_lines(FooterScope::Global, lines(lines_set));
    hub
}

#[test]
fn merge_appends_row_and_keeps_plugin_rows() {
    let hub = hub_with(&["+1 | counter | 好的"]);
    let mut plugin = Keyboard::new();
    plugin
        .append_button("plugin.btn", "插件按钮", "", ButtonStyle::Blue, 0)
        .expect("button");
    let merged = hub.merge(Some(plugin), "", Scene::Group).expect("merged");
    assert_eq!(merged.rows.len(), 2);
    assert_eq!(merged.rows[0].buttons[0].id, "plugin.btn");
    assert_eq!(merged.rows[1].buttons[0].id, "aurorix.footer.0");
}

#[test]
fn merge_creates_keyboard_when_message_has_none() {
    let hub = hub_with(&["+1 | counter | 好的"]);
    let merged = hub.merge(None, "", Scene::Group).expect("merged");
    assert_eq!(merged.rows.len(), 1);
}

#[test]
fn merge_drops_whole_footer_when_rows_do_not_all_fit() {
    let hub = hub_with(&["+1 | counter | 好的", "帮助 | command | /help"]);
    let mut nearly_full = Keyboard::new();
    for index in 0..(aurorix_buttons::MAX_ROWS - 1) {
        nearly_full
            .append_button(
                &format!("p{index}"),
                "插件",
                "",
                ButtonStyle::Blue,
                index as i32,
            )
            .expect("button");
    }
    let merged = hub
        .merge(Some(nearly_full), "", Scene::Group)
        .expect("merged");
    assert_eq!(merged.rows.len(), aurorix_buttons::MAX_ROWS - 1);
    assert!(merged.rows.iter().all(|row| row
        .buttons
        .iter()
        .all(|button| !button.id.starts_with(aurorix_buttons::FOOTER_ID_PREFIX))));
}

#[test]
fn merge_appends_every_footer_row_when_board_has_room() {
    let hub = hub_with(&["+1 | counter | 好的", "帮助 | command | /help"]);
    let mut board = Keyboard::new();
    for index in 0..(aurorix_buttons::MAX_ROWS - 2) {
        board
            .append_button(
                &format!("p{index}"),
                "插件",
                "",
                ButtonStyle::Blue,
                index as i32,
            )
            .expect("button");
    }
    let merged = hub.merge(Some(board), "", Scene::Group).expect("merged");
    assert_eq!(merged.rows.len(), aurorix_buttons::MAX_ROWS);
    assert_eq!(
        merged.rows[aurorix_buttons::MAX_ROWS - 2].buttons[0].id,
        "aurorix.footer.0"
    );
    assert_eq!(
        merged.rows[aurorix_buttons::MAX_ROWS - 1].buttons[0].id,
        "aurorix.footer.1"
    );
}

#[test]
fn merge_skips_when_board_is_full() {
    let hub = hub_with(&["+1 | counter | 好的"]);
    let mut full = Keyboard::new();
    for index in 0..aurorix_buttons::MAX_ROWS {
        full.append_button(
            &format!("p{index}"),
            "插件",
            "",
            ButtonStyle::Blue,
            index as i32,
        )
        .expect("button");
    }
    let merged = hub.merge(Some(full), "", Scene::Group).expect("merged");
    assert_eq!(merged.rows.len(), aurorix_buttons::MAX_ROWS);
    for row in &merged.rows {
        for button in &row.buttons {
            assert!(!button.id.starts_with(aurorix_buttons::FOOTER_ID_PREFIX));
        }
    }
}

#[test]
fn repeat_button_fills_original_input_and_disappears_without_it() {
    let specs = parse_footer(&lines(&["+1 | repeat"])).specs;
    let row = footer_buttons(&specs, "/p rand");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].action.action_type, ButtonAction::Command);
    assert_eq!(row[0].action.msg.as_deref(), Some("/p rand"));
    assert!(!row[0].action.auto_send);
    assert!(footer_buttons(&specs, "").is_empty());
}

#[test]
fn repeat_auto_sends_original_input() {
    let specs = parse_footer(&lines(&["+1 | repeat | | auto=1"])).specs;
    assert_eq!(specs.len(), 1);
    let row = footer_buttons(&specs, "/p rand");
    assert!(row[0].action.auto_send);
    assert_eq!(row[0].action.msg.as_deref(), Some("/p rand"));
}

#[test]
fn image_button_uses_callback_and_sent_visited_label() {
    let specs = parse_footer(&lines(&[
        "免@设置步骤 | image | https://example.com/guide.png",
    ]))
    .specs;
    let row = footer_buttons(&specs, "");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].action.action_type, ButtonAction::Callback);
    assert_eq!(row[0].action.callback_data.as_deref(), Some("0"));
    assert_eq!(row[0].render_data.visited_label, "已发送");
}

#[test]
fn image_rejects_non_url() {
    let result = parse_footer(&lines(&["免@设置步骤 | image | 不是链接"]));
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("http(s)"), "{:?}", result.errors);
}

#[test]
fn scene_scoped_buttons_win_then_fall_back_to_global() {
    let hub = hub_with(&["全局 | command | /help"]);
    hub.set_lines(
        FooterScope::SceneChannel,
        lines(&["频道专用 | command | /c"]),
    );

    let group = hub.merge(None, "", Scene::Group).expect("group");
    assert_eq!(group.rows[0].buttons[0].render_data.label, "全局");
    assert_eq!(
        group.rows[0].buttons[0].id,
        footer_button_id(FooterScope::Global.slot_base())
    );

    let channel = hub.merge(None, "", Scene::Channel).expect("channel");
    assert_eq!(channel.rows[0].buttons[0].render_data.label, "频道专用");
    assert_eq!(
        channel.rows[0].buttons[0].id,
        footer_button_id(FooterScope::SceneChannel.slot_base())
    );

    hub.set_source(FooterScope::SceneChannel, None);
    let fallback = hub.merge(None, "", Scene::Channel).expect("fallback");
    assert_eq!(fallback.rows[0].buttons[0].render_data.label, "全局");
}

#[test]
fn scoped_keyboard_uses_scope_slot_base() {
    let hub = hub_with(&["+1 | counter | 好的"]);
    hub.set_lines(FooterScope::NoticeWelcome, lines(&["欢迎 | command | /hi"]));
    let keyboard = hub
        .keyboard(FooterScope::NoticeWelcome, "")
        .expect("keyboard");
    assert_eq!(
        keyboard.rows[0].buttons[0].id,
        footer_button_id(FooterScope::NoticeWelcome.slot_base())
    );
    assert!(hub.keyboard(FooterScope::SceneDm, "").is_none());
}

#[test]
fn style_option_accepts_names_and_numeric_codes() {
    for (value, expected) in [
        ("blue", ButtonStyle::Blue),
        ("gray", ButtonStyle::Gray),
        ("0", ButtonStyle::Gray),
        ("1", ButtonStyle::Blue),
        ("2", ButtonStyle::GraySolid),
        ("3", ButtonStyle::BlueSolid),
        ("4", ButtonStyle::BlueText),
    ] {
        let line = format!("+1 | counter | {{count}} | style={value}");
        let result = parse_footer(&[line]);
        assert!(
            result.errors.is_empty(),
            "style={value} 应合法: {:?}",
            result.errors
        );
        let row = footer_buttons(&result.specs, "");
        assert_eq!(row[0].render_data.style, expected, "style={value} 映射错");
    }
    for value in ["red", "grey", "5", "-1"] {
        let result = parse_footer(&[format!("+1 | counter | x | style={value}")]);
        assert_eq!(result.errors.len(), 1, "style={value} 应被拒绝");
    }
}

#[test]
fn style_option_maps_to_button_style_codes() {
    assert_eq!(footer_style("gray"), ButtonStyle::Gray);
    assert_eq!(footer_style("  BLUE "), ButtonStyle::Blue);
    assert_eq!(footer_style("2"), ButtonStyle::GraySolid);
    assert_eq!(footer_style("3"), ButtonStyle::BlueSolid);
    assert_eq!(footer_style(""), ButtonStyle::BlueText);
}

#[test]
fn handler_ids_cover_every_scope_slot() {
    for scope in FooterScope::ALL {
        for index in 0..5 {
            let id = footer_button_id(scope.slot_base() + index);
            assert!(id.starts_with("aurorix.footer."));
        }
    }
    assert_eq!(footer_button_id(0), "aurorix.footer.0");
    assert_eq!(
        footer_button_id(FooterScope::SceneDm.slot_base() + 4),
        "aurorix.footer.34"
    );
}

#[test]
fn counter_key_is_scoped() {
    let specs = parse_footer(&lines(&["+1 | counter | 好的"])).specs;
    let key = aurorix_buttons::footer_count_key(FooterScope::SceneGroup, &specs[0]);
    assert_eq!(key, "footer.scene.group.+1");
    let global = aurorix_buttons::footer_count_key(FooterScope::Global, &specs[0]);
    assert_eq!(global, "footer.+1");
}

#[test]
fn footer_slot_of_accepts_last_slot_and_rejects_overflow() {
    assert_eq!(
        footer_slot_of(34),
        Some((FooterScope::SceneDm, 4)),
        "槽位 34 是最后一个合法槽位"
    );
    assert_eq!(footer_slot_of(35), None, "槽位 35 越界");
    assert_eq!(
        footer_slot_of(FooterScope::SceneDm.slot_base()),
        Some((FooterScope::SceneDm, 0)),
        "槽位 30 应当是频道私信的第一个按钮"
    );
    assert_eq!(
        footer_slot_of(FooterScope::Global.slot_base()),
        Some((FooterScope::Global, 0)),
        "槽位 0 应当是全局的第一个按钮"
    );
    assert_eq!(footer_slot_of(-1), None, "负槽位不合法");
}

#[test]
fn style_option_accepts_both_allowed_values() {
    for style in ["blue", "gray"] {
        let line = format!("+1 | counter | 好的 | style={style}");
        let result = parse_footer(&lines(&[line.as_str()]));
        assert!(
            result.errors.is_empty(),
            "style={style} 应当被接受: {:?}",
            result.errors
        );
        assert_eq!(result.specs.len(), 1, "style={style} 应当产出一个按钮");
        assert_eq!(result.specs[0].option("style", ""), style);
    }
    let bad = parse_footer(&lines(&["+1 | counter | 好的 | style=red"]));
    assert_eq!(bad.errors.len(), 1, "style=red 应当被拒绝");
}

#[test]
fn hub_reports_errors_only_once_per_content() {
    let hub = hub_with(&["+1 | nope"]);
    let (first, errors) = hub.current_result(FooterScope::Global);
    assert!(first.specs.is_empty(), "类型不认识的按钮不该产出");
    assert_eq!(errors.len(), 1, "首次解析应当报告一条错误: {errors:?}");
    let (second, repeated) = hub.current_result(FooterScope::Global);
    assert!(second.specs.is_empty(), "缓存结果也不该产出按钮");
    assert!(
        repeated.is_empty(),
        "内容没变时不该重复报告错误: {repeated:?}"
    );
    hub.set_lines(FooterScope::Global, lines(&["+1 | counter | 好的"]));
    let (third, after_change) = hub.current_result(FooterScope::Global);
    assert_eq!(third.specs.len(), 1, "内容变化后应当重新解析");
    assert!(after_change.is_empty(), "新内容没有错误: {after_change:?}");
}

#[test]
fn merge_appends_footer_row_after_gap_rows() {
    let hub = hub_with(&["+1 | counter | 好的"]);
    let mut plugin = Keyboard::new();
    plugin
        .append_button("plugin.zero", "插件", "", ButtonStyle::Blue, 0)
        .expect("button");
    plugin
        .append_button("plugin.two", "插件", "", ButtonStyle::Blue, 2)
        .expect("button");
    assert_eq!(plugin.rows.len(), 3, "插件键盘应当有 3 行(含空行)");
    let merged = hub.merge(Some(plugin), "", Scene::Group).expect("merged");
    assert_eq!(merged.rows.len(), 4, "底部按钮行应当被追加进去");
    let last = merged.rows.last().expect("最后一行");
    assert_eq!(last.buttons.len(), 1, "最后一行应当只有一个底部按钮");
    assert_eq!(last.buttons[0].id, "aurorix.footer.0");
    assert_eq!(merged.rows[2].buttons[0].id, "plugin.two");
}
