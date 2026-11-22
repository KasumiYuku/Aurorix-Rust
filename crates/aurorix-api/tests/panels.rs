mod support;

use aurorix_api::{
    panel_display_width, valid_panel_scope, validate_panel, Panel, PanelItem, PANEL_DESC_MAX_WIDTH,
    PANEL_ITEM_COMMAND, PANEL_ITEM_LINK, PANEL_MAX_ITEMS, PANEL_MAX_PAGES, PANEL_MAX_PER_BOT,
    PANEL_NAME_MAX_WIDTH, PANEL_PAGE_LIMIT, PANEL_REMARK_MAX_LEN, PANEL_TARGET_ALL,
};
use support::{MockResponse, MockServer};

fn item(name: &str, desc: &str) -> PanelItem {
    PanelItem {
        name: String::from(name),
        desc: String::from(desc),
        kind: String::from(PANEL_ITEM_COMMAND),
        only_admin: false,
        link: String::new(),
    }
}

fn panel_fixture() -> Panel {
    Panel {
        items: vec![
            item("签到", "每日签到一次"),
            PanelItem {
                name: String::from("帮助"),
                desc: String::from("查看帮助说明"),
                kind: String::from(PANEL_ITEM_LINK),
                only_admin: true,
                link: String::from("qq.example.com/panel"),
            },
        ],
        remark: String::new(),
        version: 0,
    }
}

#[test]
fn panel_limits_match_platform_rules() {
    assert_eq!(PANEL_NAME_MAX_WIDTH, 14, "名字显示宽度上限是 14");
    assert_eq!(PANEL_DESC_MAX_WIDTH, 30, "描述显示宽度上限是 30");
    assert_eq!(PANEL_REMARK_MAX_LEN, 255, "备注长度上限是 255");
    assert_eq!(PANEL_MAX_ITEMS, 20, "单个面板最多 20 项");
    assert_eq!(PANEL_MAX_PER_BOT, 20, "每个机器人最多 20 个面板");
    assert_eq!(PANEL_PAGE_LIMIT, 50, "每页固定拉 50 条");
    assert_eq!(PANEL_MAX_PAGES, 40, "最多翻 40 页");
    assert_eq!(PANEL_ITEM_COMMAND, "command");
    assert_eq!(PANEL_ITEM_LINK, "link");
    assert_eq!(PANEL_TARGET_ALL, "all");
}

#[test]
fn display_width_counts_cjk_and_fullwidth_as_two() {
    assert_eq!(panel_display_width(""), 0, "空串宽度是 0");
    assert_eq!(panel_display_width("abc"), 3, "ASCII 每个字符计 1");
    assert_eq!(panel_display_width("a1!"), 3, "标点也算 1");
    assert_eq!(panel_display_width("中文"), 4, "中文每个字计 2");
    assert_eq!(panel_display_width("，。！"), 6, "全角标点每个计 2");
    assert_eq!(panel_display_width("a中,b"), 5, "半角与全角混排要分别计算");
}

#[test]
fn valid_panel_scope_accepts_only_four_scenes() {
    for scope in ["group", "c2c", "channel", "dm"] {
        assert!(valid_panel_scope(scope), "{scope} 应当被接受");
    }
    for scope in ["guild", "Group", "", "friend", "group "] {
        assert!(!valid_panel_scope(scope), "{scope} 不应当被接受");
    }
}

#[test]
fn validate_panel_accepts_fixture() {
    let panel = panel_fixture();
    assert_eq!(validate_panel(&panel), Ok(()), "合法面板应当通过校验");
    let empty = Panel::default();
    assert_eq!(validate_panel(&empty), Ok(()), "空面板本身合法");
}

#[test]
fn validate_panel_rejects_too_many_items() {
    let mut panel = panel_fixture();
    panel.items = vec![item("签到", "每日签到一次"); PANEL_MAX_ITEMS + 1];
    let err = validate_panel(&panel).expect_err("超过 20 项必须被拒绝");
    assert!(err.contains("超过上限 20"), "错误要说明项数上限: {err}");
}

#[test]
fn validate_panel_rejects_long_remark() {
    let mut panel = panel_fixture();
    panel.remark = "备".repeat(PANEL_REMARK_MAX_LEN + 1);
    let err = validate_panel(&panel).expect_err("备注超过 255 字必须被拒绝");
    assert!(
        err.contains("备注长度 256"),
        "错误要说明备注实际长度: {err}"
    );
}

#[test]
fn validate_panel_rejects_blank_name_and_desc() {
    let mut blank_name = panel_fixture();
    blank_name.items[1].name = String::from("   ");
    let err = validate_panel(&blank_name).expect_err("名字只有空格必须被拒绝");
    assert!(err.contains("第 2 项"), "错误要指明第几项: {err}");
    assert!(err.contains("名字不能为空"), "错误要说明名字为空: {err}");

    let mut blank_desc = panel_fixture();
    blank_desc.items[1].desc = String::new();
    let err = validate_panel(&blank_desc).expect_err("描述为空必须被拒绝");
    assert!(err.contains("第 2 项"), "错误要指明第几项: {err}");
    assert!(err.contains("描述不能为空"), "错误要说明描述为空: {err}");
}

#[test]
fn validate_panel_rejects_oversized_text() {
    let mut long_name = panel_fixture();
    long_name.items[1].name = "名".repeat(PANEL_NAME_MAX_WIDTH / 2 + 1);
    let err = validate_panel(&long_name).expect_err("名字宽度超过 14 必须被拒绝");
    assert!(err.contains("第 2 项"), "错误要指明第几项: {err}");
    assert!(err.contains("显示宽度 16"), "错误要说明实际宽度: {err}");
    assert!(err.contains("超过上限 14"), "错误要说明名称宽度上限: {err}");

    let mut long_desc = panel_fixture();
    long_desc.items[0].desc = "述".repeat(PANEL_DESC_MAX_WIDTH / 2 + 1);
    let err = validate_panel(&long_desc).expect_err("描述宽度超过 30 必须被拒绝");
    assert!(err.contains("第 1 项"), "错误要指明第几项: {err}");
    assert!(err.contains("显示宽度 32"), "错误要说明实际宽度: {err}");
    assert!(err.contains("超过上限 30"), "错误要说明描述宽度上限: {err}");
}

#[tokio::test]
async fn panel_list_follows_cursor_until_end() {
    let server = MockServer::with_script(vec![
        MockResponse::json(
            r#"{"records":[{"panel_id":"p-1","scope":"group"}],"next_cursor":"cur-2","is_end":false}"#,
        ),
        MockResponse::json(
            r#"{"records":[{"panel_id":"p-2","scope":"group"}],"next_cursor":"","is_end":true}"#,
        ),
    ])
    .await;
    let api = server.api();
    let records = api.panel_list("group").await.expect("拉取指令面板应当成功");
    let ids: Vec<&str> = records
        .iter()
        .map(|record| record.panel_id.as_str())
        .collect();
    assert_eq!(ids, vec!["p-1", "p-2"], "两页结果要按顺序合并");
    let requests = server.api_requests();
    assert_eq!(requests.len(), 2, "翻两页要请求两次");
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v2/panels");
    assert!(
        requests[0].url().contains("scope=group"),
        "首页要带场景: {}",
        requests[0].url()
    );
    assert!(
        requests[0].url().contains("limit=50"),
        "首页要带分页大小: {}",
        requests[0].url()
    );
    assert!(
        !requests[0].url().contains("cursor"),
        "首页不该带游标: {}",
        requests[0].url()
    );
    assert!(
        requests[1].url().contains("cursor=cur-2"),
        "第二页要带游标: {}",
        requests[1].url()
    );
}

#[tokio::test]
async fn panel_list_rejects_bad_scope() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let err = api
        .panel_list("guild")
        .await
        .expect_err("非法的场景必须被拒绝");
    assert!(
        err.to_string().contains("不合法"),
        "错误要说明场景非法: {err}"
    );
    assert_eq!(server.api_count(), 0, "非法场景不该发出请求");
}

#[tokio::test]
async fn panel_count_sums_four_scenes() {
    let server = MockServer::with_script(vec![
        MockResponse::json(r#"{"records":[{"panel_id":"p-1"}],"is_end":true}"#),
        MockResponse::json(r#"{"records":[],"is_end":true}"#),
        MockResponse::json(r#"{"records":[{"panel_id":"p-2"},{"panel_id":"p-3"}],"is_end":true}"#),
        MockResponse::json(r#"{"records":[],"is_end":true}"#),
    ])
    .await;
    let api = server.api();
    let total = api.panel_count().await.expect("统计面板数量应当成功");
    assert_eq!(total, 3, "四个场景的结果要相加");
    assert_eq!(server.api_count(), 4, "四个场景各查一次");
}

#[tokio::test]
async fn panel_get_reads_single_record() {
    let server = MockServer::with_script(vec![MockResponse::json(
        r#"{"panel_id":"p-9","scope":"c2c","target_type":"all","version":4,"panel":{"items":[{"name":"签到","desc":"每日一次","type":"command"}]}}"#,
    )])
    .await;
    let api = server.api();
    let record = api.panel_get("p-9").await.expect("读取指令面板应当成功");
    assert_eq!(record.panel_id, "p-9");
    assert_eq!(record.scope, "c2c");
    assert_eq!(record.version, 4);
    assert_eq!(record.panel.items.len(), 1);
    assert_eq!(record.panel.items[0].name, "签到");
    assert_eq!(record.panel.items[0].kind, "command");
    let request = server.last();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/v2/panels/p-9");
    assert!(request.body.is_empty(), "读取请求不该带报文");
}

#[tokio::test]
async fn panel_create_posts_body_and_reads_both_shapes() {
    let server = MockServer::with_script(vec![
        MockResponse::json(r#"{"panel_id":"p-1"}"#),
        MockResponse::json(r#"{"data":{"panel_id":"p-2"}}"#),
    ])
    .await;
    let api = server.api();
    let panel = panel_fixture();
    let first = api
        .panel_create("group", &panel)
        .await
        .expect("创建指令面板应当成功");
    assert_eq!(first, "p-1", "顶层形状要能取到 panel_id");
    let second = api
        .panel_create("c2c", &panel)
        .await
        .expect("创建指令面板应当成功");
    assert_eq!(second, "p-2", "data 包裹形状也要能取到 panel_id");
    let requests = server.api_requests();
    assert_eq!(requests.len(), 2, "两次创建各发一次请求");
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v2/panels");
    let body = requests[0].json();
    assert_eq!(body["scope"], "group");
    assert_eq!(body["target_type"], PANEL_TARGET_ALL);
    assert_eq!(body["panel"]["items"][0]["name"], "签到");
    assert_eq!(body["panel"]["items"][0]["desc"], "每日签到一次");
    assert_eq!(body["panel"]["items"][0]["type"], PANEL_ITEM_COMMAND);
    assert_eq!(body["panel"]["items"][1]["type"], PANEL_ITEM_LINK);
    assert_eq!(
        body["panel"]["items"][1]["only_admin"].as_bool(),
        Some(true),
        "only_admin 为真时要提交"
    );
    assert!(
        body["panel"]["items"][0].get("only_admin").is_none(),
        "only_admin 为假时不该提交: {body}"
    );
    assert!(
        body["panel"]["items"][0].get("link").is_none(),
        "链接为空时不该提交: {body}"
    );
    assert_eq!(
        body["panel"]["items"][1]["link"], "qq.example.com/panel",
        "带链接的指令项要原样提交"
    );
    assert!(
        body["panel"].get("remark").is_none(),
        "备注为空时不该提交: {body}"
    );
    assert!(
        body["panel"].get("version").is_none(),
        "版本为 0 时不该提交: {body}"
    );
    assert_eq!(requests[1].json()["scope"], "c2c");
}

#[tokio::test]
async fn panel_create_rejects_invalid_panel() {
    let server = MockServer::with_script(Vec::new()).await;
    let api = server.api();
    let mut panel = panel_fixture();
    panel.items[0].name = "名".repeat(PANEL_NAME_MAX_WIDTH / 2 + 1);
    let err = api
        .panel_create("group", &panel)
        .await
        .expect_err("本地校验不通过必须直接报错");
    assert!(
        err.to_string().contains("显示宽度 16"),
        "错误要说明实际宽度: {err}"
    );
    assert_eq!(server.api_count(), 0, "校验失败不该发出请求");
}

#[tokio::test]
async fn panel_update_zeroes_version_and_returns_new_one() {
    let server = MockServer::with_script(vec![
        MockResponse::json(r#"{"version":7}"#),
        MockResponse::json(r#"{"data":{"version":8}}"#),
    ])
    .await;
    let api = server.api();
    let mut panel = panel_fixture();
    panel.version = 3;
    let first = api
        .panel_update("p-1", &panel)
        .await
        .expect("更新指令面板应当成功");
    assert_eq!(first, 7, "顶层形状要能取到 version");
    let second = api
        .panel_update("p-1", &panel)
        .await
        .expect("更新指令面板应当成功");
    assert_eq!(second, 8, "data 包裹形状也要能取到 version");
    let requests = server.api_requests();
    assert_eq!(requests.len(), 2, "两次更新各发一次请求");
    assert_eq!(requests[0].method, "PUT");
    assert_eq!(requests[0].path, "/v2/panels/p-1");
    let body = requests[0].json();
    assert!(
        body["panel"].get("version").is_none(),
        "提交时版本号必须归零: {body}"
    );
    assert_eq!(body["panel"]["items"][0]["name"], "签到");
    assert!(body.get("scope").is_none(), "更新请求不该带场景: {body}");
    assert_eq!(panel.version, 3, "调用方的面板不该被改动");
}

#[tokio::test]
async fn panel_delete_uses_delete_method() {
    let server = MockServer::with_script(vec![MockResponse::json("{}")]).await;
    let api = server.api();
    api.panel_delete("p-1").await.expect("删除指令面板应当成功");
    let requests = server.api_requests();
    assert_eq!(requests.len(), 1, "删除只发一次请求");
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(requests[0].path, "/v2/panels/p-1");
    assert!(requests[0].body.is_empty(), "删除请求不该带报文");
}
