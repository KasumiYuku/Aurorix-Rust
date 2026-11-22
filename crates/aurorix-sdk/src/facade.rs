use aurorix_buttons::Builder;
use aurorix_template::{Args, Registry, TemplateKind};

use crate::error::SdkError;

pub fn render(registry: &Registry, id: &str, args: Args) -> Result<String, SdkError> {
    registry
        .fill(TemplateKind::Markdown, "", id, &args)
        .map_err(SdkError::from)
}

pub fn render_for(registry: &Registry, ns: &str, id: &str, args: Args) -> Result<String, SdkError> {
    registry
        .fill(TemplateKind::Markdown, ns, id, &args)
        .map_err(SdkError::from)
}

pub fn keyboard(plugin_id: &str) -> Builder {
    Builder::new().scope(plugin_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurorix_template::args_map;

    fn registry() -> Registry {
        let registry = Registry::new();
        registry.register(TemplateKind::Markdown, "", "hello", "你好 {{name}}");
        registry.register(
            TemplateKind::Markdown,
            "arcaea",
            "b50",
            "你的 b50 是 {{score}}",
        );
        registry
    }

    #[test]
    fn render_fills_global_template() {
        let registry = registry();
        let text =
            render(&registry, "hello", args_map! { "name" => "小明" }).expect("模板要渲染成功");
        assert_eq!(text, "你好 小明");
    }

    #[test]
    fn render_for_prefers_namespace() {
        let registry = registry();
        let text = render_for(&registry, "arcaea", "b50", args_map! { "score" => "12.34" })
            .expect("模板要渲染成功");
        assert_eq!(text, "你的 b50 是 12.34");
        let fallback = render_for(&registry, "other", "hello", args_map! { "name" => "小红" })
            .expect("命名空间缺失要回退到全局");
        assert_eq!(fallback, "你好 小红");
    }

    #[test]
    fn missing_template_wraps_template_error() {
        let registry = registry();
        let err = render(&registry, "absent", Args::new()).unwrap_err();
        assert!(
            matches!(err, SdkError::Template(_)),
            "缺失模板要包成模板错误"
        );
        assert_eq!(
            err.to_string(),
            "模板渲染失败: markdown 模板 absent 不存在(已查 全局命名空间)"
        );
        let scoped = render_for(&registry, "arcaea", "absent", Args::new()).unwrap_err();
        assert!(matches!(scoped, SdkError::Template(_)));
    }

    #[test]
    fn keyboard_applies_plugin_scope() {
        let built = keyboard("arcaea")
            .command("refresh", "刷新", "/a b50", Vec::new())
            .build()
            .expect("按钮要能构建");
        assert_eq!(built.rows[0].buttons[0].id, "arcaea.refresh");
        assert_eq!(built.rows[0].buttons[0].render_data.label, "刷新");
    }
}
