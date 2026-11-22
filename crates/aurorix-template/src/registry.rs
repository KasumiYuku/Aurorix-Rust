use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::LazyLock;

use arc_swap::ArcSwap;

use crate::args::Args;
use crate::engine::{fill as fill_body, process_template};
use crate::error::{TemplateError, TemplateResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TemplateKind {
    Markdown,
    Html,
}

impl TemplateKind {
    pub const fn suffix(self) -> &'static str {
        match self {
            TemplateKind::Markdown => ".md",
            TemplateKind::Html => ".html",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TemplateKind::Markdown => "markdown",
            TemplateKind::Html => "html",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Template {
    id: String,
    body: String,
    args: Vec<String>,
    kind: TemplateKind,
}

impl Template {
    pub fn new(kind: TemplateKind, id: &str, content: &str) -> Self {
        let processed = process_template(content);
        Self {
            id: id.to_string(),
            body: processed.body,
            args: processed.args,
            kind,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn kind(&self) -> TemplateKind {
        self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TemplateKey {
    kind: TemplateKind,
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BaseKey {
    pub(crate) ns: String,
    pub(crate) kind: TemplateKind,
    pub(crate) id: String,
}

#[derive(Debug, Default, Clone)]
struct State {
    templates: BTreeMap<String, BTreeMap<TemplateKey, Arc<Template>>>,
    base: BTreeMap<BaseKey, Arc<str>>,
}

pub struct Registry {
    state: ArcSwap<State>,
    write_lock: std::sync::Mutex<()>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("namespaces", &self.state.load().templates.len())
            .finish()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            state: ArcSwap::from_pointee(State::default()),
            write_lock: std::sync::Mutex::new(()),
        }
    }

    pub fn global() -> &'static Registry {
        static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);
        &REGISTRY
    }

    pub fn register(&self, kind: TemplateKind, ns: &str, id: &str, content: &str) -> bool {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = State::clone(&self.state.load_full());
        next.base.insert(
            BaseKey {
                ns: ns.to_string(),
                kind,
                id: id.to_string(),
            },
            Arc::from(content),
        );
        let duplicated = Self::write(&mut next, kind, ns, id, content);
        self.state.store(Arc::new(next));
        duplicated
    }

    pub(crate) fn put(&self, kind: TemplateKind, ns: &str, id: &str, content: &str) {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = State::clone(&self.state.load_full());
        Self::write(&mut next, kind, ns, id, content);
        self.state.store(Arc::new(next));
    }

    fn write(state: &mut State, kind: TemplateKind, ns: &str, id: &str, content: &str) -> bool {
        let template = Arc::new(Template::new(kind, id, content));
        let key = TemplateKey {
            kind,
            id: id.to_string(),
        };
        state
            .templates
            .entry(ns.to_string())
            .or_default()
            .insert(key, template)
            .is_some()
    }

    pub fn drop_template(&self, kind: TemplateKind, ns: &str, id: &str) {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = State::clone(&self.state.load_full());
        let key = TemplateKey {
            kind,
            id: id.to_string(),
        };
        let removed = next
            .templates
            .get_mut(ns)
            .map(|bucket| bucket.remove(&key).is_some())
            .unwrap_or(false);
        if removed {
            self.state.store(Arc::new(next));
        }
    }

    pub fn base_of(&self, kind: TemplateKind, ns: &str, id: &str) -> Option<Arc<str>> {
        let state = self.state.load_full();
        state
            .base
            .get(&BaseKey {
                ns: ns.to_string(),
                kind,
                id: id.to_string(),
            })
            .cloned()
    }

    pub fn exists(&self, kind: TemplateKind, id: &str) -> bool {
        let state = self.state.load_full();
        state.templates.values().any(|bucket| {
            bucket.contains_key(&TemplateKey {
                kind,
                id: id.to_string(),
            })
        })
    }

    pub fn count(&self, kind: TemplateKind) -> usize {
        let state = self.state.load_full();
        state
            .templates
            .values()
            .map(|bucket| bucket.keys().filter(|key| key.kind == kind).count())
            .sum()
    }

    pub fn ids(&self, kind: TemplateKind, ns: &str) -> Vec<String> {
        let state = self.state.load_full();
        state
            .templates
            .get(ns)
            .map(|bucket| {
                bucket
                    .keys()
                    .filter(|key| key.kind == kind)
                    .map(|key| key.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get(&self, kind: TemplateKind, ns: &str, id: &str) -> Option<Arc<Template>> {
        let state = self.state.load_full();
        state.templates.get(ns).and_then(|bucket| {
            bucket
                .get(&TemplateKey {
                    kind,
                    id: id.to_string(),
                })
                .cloned()
        })
    }

    pub fn fill(
        &self,
        kind: TemplateKind,
        ns: &str,
        id: &str,
        args: &Args,
    ) -> TemplateResult<String> {
        if !ns.is_empty() {
            if let Some(template) = self.get(kind, ns, id) {
                return fill_body(&template.body, args);
            }
        }
        if let Some(template) = self.get(kind, "", id) {
            return fill_body(&template.body, args);
        }
        let scope = if ns.is_empty() {
            "全局命名空间".to_string()
        } else {
            format!("命名空间 {ns:?} 与全局命名空间")
        };
        Err(TemplateError::TemplateNotFound {
            kind: kind.label(),
            id: id.to_string(),
            scope,
        })
    }

    pub fn register_files<I, N, C>(&self, kind: TemplateKind, ns: &str, files: I) -> usize
    where
        I: IntoIterator<Item = (N, C)>,
        N: AsRef<str>,
        C: AsRef<str>,
    {
        let mut registered = 0;
        for (name, content) in files {
            let name = name.as_ref();
            if !name.ends_with(kind.suffix()) {
                continue;
            }
            let base = name.rsplit('/').next().unwrap_or(name);
            let id = base.trim_end_matches(kind.suffix());
            self.register(kind, ns, id, content.as_ref());
            registered += 1;
        }
        registered
    }
}

pub fn new_markdown_template(id: &str, content: &str) {
    Registry::global().register(TemplateKind::Markdown, "", id, content);
}

pub fn new_html_template(id: &str, content: &str) {
    Registry::global().register(TemplateKind::Html, "", id, content);
}

pub const BUILTIN_CARD_TEMPLATE: &str = include_str!("../assets/builtin/Card.md");

pub fn register_builtin_templates(registry: &Registry) -> usize {
    if !registry.register(TemplateKind::Markdown, "", "Card", BUILTIN_CARD_TEMPLATE) {
        return 0;
    }
    1
}

pub fn is_markdown_template_exist(id: &str) -> bool {
    Registry::global().exists(TemplateKind::Markdown, id)
}

pub fn is_html_template_exist(id: &str) -> bool {
    Registry::global().exists(TemplateKind::Html, id)
}

pub fn fill_markdown_template(id: &str, args: &Args) -> TemplateResult<String> {
    Registry::global().fill(TemplateKind::Markdown, "", id, args)
}

pub fn fill_for(ns: &str, id: &str, args: &Args) -> TemplateResult<String> {
    Registry::global().fill(TemplateKind::Markdown, ns, id, args)
}

pub fn fill_html_template(id: &str, args: &Args) -> TemplateResult<String> {
    Registry::global().fill(TemplateKind::Html, "", id, args)
}

pub fn fill_html_for(ns: &str, id: &str, args: &Args) -> TemplateResult<String> {
    Registry::global().fill(TemplateKind::Html, ns, id, args)
}

pub fn markdown_template_count() -> usize {
    Registry::global().count(TemplateKind::Markdown)
}

pub fn html_template_count() -> usize {
    Registry::global().count(TemplateKind::Html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args_map;

    fn registry() -> Registry {
        Registry::new()
    }

    #[test]
    fn register_and_fill_global_template() {
        let reg = registry();
        assert!(!reg.register(TemplateKind::Markdown, "", "hello", "你好 {{name}}"));
        let args = args_map! { "name" => "小明" };
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "hello", &args)
                .unwrap(),
            "你好 小明"
        );
    }

    #[test]
    fn duplicate_registration_reports_overwrite() {
        let reg = registry();
        assert!(!reg.register(TemplateKind::Markdown, "", "dup", "a"));
        assert!(reg.register(TemplateKind::Markdown, "", "dup", "b"));
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "dup", &Args::new())
                .unwrap(),
            "b"
        );
    }

    #[test]
    fn namespace_wins_over_global_and_falls_back() {
        let reg = registry();
        reg.register(TemplateKind::Html, "", "page", "<p>全局</p>");
        reg.register(TemplateKind::Html, "plugin", "page", "<p>插件</p>");
        assert_eq!(
            reg.fill(TemplateKind::Html, "plugin", "page", &Args::new())
                .unwrap(),
            "<p>插件</p>"
        );
        assert_eq!(
            reg.fill(TemplateKind::Html, "other", "page", &Args::new())
                .unwrap(),
            "<p>全局</p>"
        );
    }

    #[test]
    fn missing_template_reports_kind_and_scope() {
        let reg = registry();
        let err = reg
            .fill(TemplateKind::Html, "", "absent", &Args::new())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "html 模板 absent 不存在(已查 全局命名空间)"
        );
        let scoped = reg
            .fill(TemplateKind::Html, "ns-x", "absent", &Args::new())
            .unwrap_err();
        assert_eq!(
            scoped.to_string(),
            "html 模板 absent 不存在(已查 命名空间 \"ns-x\" 与全局命名空间)"
        );
    }

    #[test]
    fn kinds_do_not_collide() {
        let reg = registry();
        reg.register(TemplateKind::Markdown, "", "same", "md:{{v}}");
        reg.register(TemplateKind::Html, "", "same", "<p>html:{{v}}</p>");
        let args = args_map! { "v" => "1" };
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "same", &args).unwrap(),
            "md:1"
        );
        assert_eq!(
            reg.fill(TemplateKind::Html, "", "same", &args).unwrap(),
            "<p>html:1</p>"
        );
        assert_eq!(reg.count(TemplateKind::Markdown), 1);
        assert_eq!(reg.count(TemplateKind::Html), 1);
        assert!(!reg.exists(TemplateKind::Html, "missing"));
    }

    #[test]
    fn file_registration_filters_suffix_and_strips_dirs() {
        let reg = registry();
        let count = reg.register_files(
            TemplateKind::Html,
            "ns",
            [
                ("templates/html/Mail.html", "<p>你好 {{name}}</p>"),
                ("templates/html/NotHtml.md", "这是 markdown"),
            ],
        );
        assert_eq!(count, 1);
        assert_eq!(reg.ids(TemplateKind::Html, "ns"), vec!["Mail".to_string()]);
        assert_eq!(
            reg.fill(
                TemplateKind::Html,
                "ns",
                "Mail",
                &args_map! { "name" => "小明" }
            )
            .unwrap(),
            "<p>你好 小明</p>"
        );
    }

    #[test]
    fn drop_removes_template() {
        let reg = registry();
        reg.register(TemplateKind::Markdown, "ns", "gone", "x");
        assert!(reg.exists(TemplateKind::Markdown, "gone"));
        reg.drop_template(TemplateKind::Markdown, "ns", "gone");
        assert!(!reg.exists(TemplateKind::Markdown, "gone"));
        reg.drop_template(TemplateKind::Markdown, "ns", "gone");
    }

    #[test]
    fn template_args_are_exposed_for_admin_listing() {
        let template = Template::new(TemplateKind::Markdown, "card", "{{title}} {{inline a b}}");
        assert_eq!(
            template.args(),
            ["title".to_string(), "a".to_string(), "b".to_string()]
        );
        assert_eq!(template.kind(), TemplateKind::Markdown);
        assert_eq!(template.id(), "card");
    }
}
