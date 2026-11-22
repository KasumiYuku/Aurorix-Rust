use std::collections::BTreeMap;

use aurorix_template::{Arg, Args, Registry, TemplateKind};

use crate::reply::{Outgoing, ReplyKind};

pub const ERROR_CARD_TEMPLATE: &str = "error_card";
pub const FALLBACK_CARD_TEMPLATE: &str = "Card";
pub const ARGUMENT_ERROR_TITLE: &str = "❌ 指令参数错误";
pub const HANDLER_ERROR_TITLE: &str = "❌ 指令执行失败";
pub const REASON_LABEL: &str = "原因";
pub const USAGE_LABEL: &str = "用法";

fn card_args(title: &str, fields: &[(String, String)]) -> Args {
    let mut args = Args::new();
    args.insert("title".to_string(), Arg::Str(title.to_string()));
    let mut items = Vec::new();
    for (label, content) in fields {
        let mut item = BTreeMap::new();
        item.insert("label".to_string(), Arg::Str(label.clone()));
        item.insert("content".to_string(), Arg::Str(content.clone()));
        items.push(Arg::Map(item));
    }
    args.insert("fields".to_string(), Arg::List(items));
    args
}

pub fn render_card(
    templates: &Registry,
    namespace: &str,
    title: &str,
    fields: &[(String, String)],
) -> String {
    let args = card_args(title, fields);
    if let Ok(text) =
        aurorix_sdk::render_for(templates, namespace, ERROR_CARD_TEMPLATE, args.clone())
    {
        return text;
    }
    if let Ok(text) = templates.fill(TemplateKind::Markdown, "", FALLBACK_CARD_TEMPLATE, &args) {
        return text;
    }
    plain_card(title, fields)
}

pub fn plain_card(title: &str, fields: &[(String, String)]) -> String {
    let mut lines = vec![title.to_string()];
    for (label, content) in fields {
        lines.push(format!("{label}: {content}"));
    }
    lines.join("\n")
}

pub fn argument_error(
    templates: &Registry,
    namespace: &str,
    reason: &str,
    usage: &str,
) -> Outgoing {
    let fields = vec![
        (REASON_LABEL.to_string(), reason.to_string()),
        (USAGE_LABEL.to_string(), usage.to_string()),
    ];
    Outgoing::new(
        ReplyKind::ArgumentError,
        render_card(templates, namespace, ARGUMENT_ERROR_TITLE, &fields),
    )
}

pub fn handler_failure(templates: &Registry, namespace: &str, reason: &str) -> Outgoing {
    let fields = vec![(REASON_LABEL.to_string(), reason.to_string())];
    Outgoing::new(
        ReplyKind::Handler,
        render_card(templates, namespace, HANDLER_ERROR_TITLE, &fields),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurorix_template::TemplateKind;

    #[test]
    fn missing_template_falls_back_to_plain_text() {
        let templates = Registry::new();
        let reply = argument_error(&templates, "demo", "unexpected argument b", "/echo <name>");
        assert_eq!(reply.kind, ReplyKind::ArgumentError);
        assert!(reply.body().contains(ARGUMENT_ERROR_TITLE), "要有标题");
        assert!(reply.body().contains("unexpected argument b"), "要有原因");
        assert!(reply.body().contains("/echo <name>"), "要有用法");
        assert!(!reply.body().trim().is_empty());
    }

    #[test]
    fn registered_template_wins() {
        let templates = Registry::new();
        templates.register(
            TemplateKind::Markdown,
            "demo",
            ERROR_CARD_TEMPLATE,
            "卡片 {{title}}|{{#each fields}}{{label}}={{content}};{{/each}}",
        );
        let text = render_card(
            &templates,
            "demo",
            "标题",
            &[("原因".to_string(), "太多了".to_string())],
        );
        assert_eq!(text, "卡片 标题|原因=太多了;");
    }

    #[test]
    fn global_card_template_is_the_second_choice() {
        let templates = Registry::new();
        templates.register(
            TemplateKind::Markdown,
            "",
            FALLBACK_CARD_TEMPLATE,
            "兜底 {{title}}",
        );
        let text = render_card(&templates, "demo", "标题", &[]);
        assert_eq!(text, "兜底 标题");
    }

    #[test]
    fn broken_template_still_produces_text() {
        let templates = Registry::new();
        templates.register(
            TemplateKind::Markdown,
            "demo",
            ERROR_CARD_TEMPLATE,
            "坏 {{missing}}",
        );
        let text = render_card(&templates, "demo", "标题", &[]);
        assert!(text.contains("标题"), "坏模板要回落纯文本");
    }

    #[test]
    fn handler_failure_has_title_and_reason() {
        let templates = Registry::new();
        let reply = handler_failure(&templates, "demo", "指令执行失败: 炸了");
        assert_eq!(reply.kind, ReplyKind::Handler);
        assert!(reply.body().contains(HANDLER_ERROR_TITLE));
        assert!(reply.body().contains("炸了"));
    }
}
