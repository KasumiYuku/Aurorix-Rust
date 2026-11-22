use aurorix_template::{
    args_map, encode_value, fill, fill_for, fill_html_template, fill_markdown_template,
    new_html_template, new_markdown_template, process_template, Registry, TemplateKind,
    INLINE_TEXT_LIMIT,
};

fn inline_link(label: &str, command: &str, reference: bool) -> String {
    let mut out = format!(
        "<qqbot-cmd-input text=\"{}\" show=\"{}\"",
        encode_value(command),
        encode_value(label)
    );
    if reference {
        out.push_str(" reference=\"true\"");
    }
    out + " />"
}

#[test]
fn go_case_fill_markdown_template_nested() {
    let registry = Registry::new();
    registry.register(
        TemplateKind::Markdown,
        "",
        "test-nested",
        "{{user.name}} 有 {{user.items.#0}} 和 {{user.items.#1}}，共 {{count}} 个",
    );
    let args = args_map! {
        "user" => aurorix_template::Arg::Map(args_map! {
            "name" => "阿明",
            "items" => aurorix_template::Arg::List(vec!["剑".into(), "盾".into()]),
        }),
        "count" => 2i64,
    };
    assert_eq!(
        registry
            .fill(TemplateKind::Markdown, "", "test-nested", &args)
            .unwrap(),
        "阿明 有 剑 和 盾，共 2 个"
    );
}

#[test]
fn go_case_fill_markdown_template_missing_arg() {
    new_markdown_template("parity-missing", "你好 {{name}}");
    assert!(fill_markdown_template("parity-missing", &args_map! {}).is_err());
}

#[test]
fn go_case_inline_literal_identifier_and_mixed() {
    new_markdown_template(
        "parity-inline-literal",
        r#"{{inline "点击领取猫娘" "嘿嘿"}}"#,
    );
    assert_eq!(
        fill_markdown_template("parity-inline-literal", &args_map! {}).unwrap(),
        inline_link("点击领取猫娘", "嘿嘿", false)
    );

    new_markdown_template(
        "parity-inline-args",
        "前往 {{inline action.label action.command}}",
    );
    let args = args_map! {
        "action" => aurorix_template::Arg::Map(args_map! {
            "label" => "点[我]",
            "command" => "/p b30 #1",
        }),
    };
    assert_eq!(
        fill_markdown_template("parity-inline-args", &args).unwrap(),
        format!("前往 {}", inline_link("点[我]", "/p b30 #1", false))
    );

    new_markdown_template("parity-inline-mixed", r#"{{inline "查看帮助" command}}"#);
    assert_eq!(
        fill_markdown_template("parity-inline-mixed", &args_map! { "command" => "/help" }).unwrap(),
        inline_link("查看帮助", "/help", false)
    );
}

#[test]
fn go_case_inline_inside_each_block() {
    new_markdown_template(
        "parity-inline-each",
        "{{#each rows}}\n- {{inline label command}}\n{{/each}}",
    );
    let args = args_map! {
        "rows" => aurorix_template::Arg::List(vec![
            aurorix_template::Arg::Map(args_map! { "label" => "A", "command" => "/a 1" }),
            aurorix_template::Arg::Map(args_map! { "label" => "B", "command" => "/b 2" }),
        ]),
    };
    assert_eq!(
        fill_markdown_template("parity-inline-each", &args).unwrap(),
        format!(
            "\n- {}\n\n- {}\n",
            inline_link("A", "/a 1", false),
            inline_link("B", "/b 2", false)
        )
    );
}

#[test]
fn go_case_inline_reference_flag() {
    new_markdown_template("parity-ref", r#"{{inline "引用" "/p me" "true"}}"#);
    assert_eq!(
        fill_markdown_template("parity-ref", &args_map! {}).unwrap(),
        inline_link("引用", "/p me", true)
    );
    new_markdown_template("parity-ref-false", r#"{{inline "标签" "/p me" "false"}}"#);
    assert_eq!(
        fill_markdown_template("parity-ref-false", &args_map! {}).unwrap(),
        inline_link("标签", "/p me", false)
    );
}

#[test]
fn go_case_inline_missing_arg_and_too_long() {
    new_markdown_template("parity-inline-missing", "{{inline label command}}");
    let err = fill_markdown_template("parity-inline-missing", &args_map! { "label" => "点我" })
        .unwrap_err();
    assert!(err.to_string().contains("inline arg command not found"));

    let long = "a".repeat(INLINE_TEXT_LIMIT + 1);
    new_markdown_template(
        "parity-inline-toolong",
        &format!(r#"{{{{inline "标签" "{long}"}}}}"#),
    );
    let rendered = fill_markdown_template("parity-inline-toolong", &args_map! {}).unwrap();
    assert!(rendered.contains("qqbot-cmd-input"));
}

#[test]
fn go_case_builtin_card_template_renders() {
    let card = include_str!("../assets/builtin/Card.md");
    let registry = Registry::new();
    registry.register(TemplateKind::Markdown, "", "Card", card);
    let args = args_map! {
        "title" => "标题",
        "fields" => aurix_fields(),
    };
    let rendered = registry
        .fill(TemplateKind::Markdown, "", "Card", &args)
        .unwrap();
    assert!(rendered.contains("### 标题"));
    assert!(rendered.contains("名称:\n\n``` text\n内容\n```"));
}

fn aurix_fields() -> aurorix_template::Arg {
    aurorix_template::Arg::List(vec![aurorix_template::Arg::Map(args_map! {
        "label" => "名称",
        "content" => "内容",
    })])
}

#[test]
fn go_case_html_namespace_priority_and_kind_isolation() {
    new_html_template("parity-html-priority", "<p>全局</p>");
    let registry = Registry::global();
    registry.register(
        TemplateKind::Html,
        "parity-ns",
        "parity-html-priority",
        "<p>插件</p>",
    );
    assert!(fill_for("parity-ns", "parity-html-priority", &args_map! {}).is_err());
    assert_eq!(
        registry
            .fill(
                TemplateKind::Html,
                "parity-ns",
                "parity-html-priority",
                &args_map! {}
            )
            .unwrap(),
        "<p>插件</p>"
    );
    assert_eq!(
        fill_html_template("parity-html-priority", &args_map! {}).unwrap(),
        "<p>全局</p>"
    );
}

#[test]
fn go_case_processing_normalises_builtin_card_whitespace() {
    let processed = process_template(include_str!("../assets/builtin/Card.md"));
    assert!(processed.body.contains("{{title}}"));
    assert!(processed.body.contains("{{label}}"));
    assert_eq!(processed.args, vec!["title".to_string()]);
}

#[test]
fn registry_direct_fill_matches_global_helper() {
    let registry = Registry::new();
    registry.register(TemplateKind::Markdown, "", "direct", "{{v}}");
    assert_eq!(
        registry
            .fill(
                TemplateKind::Markdown,
                "",
                "direct",
                &args_map! { "v" => "x" }
            )
            .unwrap(),
        fill("{{v}}", &args_map! { "v" => "x" }).unwrap()
    );
}
