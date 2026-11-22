use std::fs;
use std::path::{Path, PathBuf};

pub const API_DIR: &str = "docs/api";

const ITEM_KEYWORDS: [&str; 8] = [
    "fn ", "struct ", "enum ", "trait ", "type ", "const ", "static ", "mod ",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiItem {
    pub kind: ItemKind,
    pub text: String,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Decl,
    Impl,
}

#[derive(Debug, Default)]
pub struct FileApi {
    pub path: String,
    pub items: Vec<ApiItem>,
}

pub struct CrateApi {
    pub name: String,
    pub files: Vec<FileApi>,
}

impl CrateApi {
    pub fn item_count(&self) -> usize {
        self.files.iter().map(|file| file.items.len()).sum()
    }
}

pub fn scan_crate(dir_name: &str, dir: &Path) -> CrateApi {
    let mut files = Vec::new();
    collect(dir.join("src").as_path(), &mut files);
    files.sort();
    let mut api = CrateApi {
        name: dir_name.to_string(),
        files: Vec::new(),
    };
    for file in files {
        let Ok(src) = fs::read_to_string(&file) else {
            continue;
        };
        let rel = file
            .strip_prefix(dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let items = extract(&src);
        if items.is_empty() {
            continue;
        }
        api.files.push(FileApi { path: rel, items });
    }
    api
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect(&path, out);
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

pub fn extract(src: &str) -> Vec<ApiItem> {
    let mut items: Vec<ApiItem> = Vec::new();
    let mut owner: Option<String> = None;
    let mut pending_use = String::new();
    let mut trait_impl = false;
    let mut export_macro = false;
    for raw in src.lines() {
        let line = raw.trim_end();
        if !pending_use.is_empty() {
            pending_use.push(' ');
            pending_use.push_str(line.trim());
            if line.trim_end().ends_with(';') {
                items.push(ApiItem {
                    kind: ItemKind::Decl,
                    text: tidy(&pending_use),
                    owner: None,
                });
                pending_use.clear();
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("impl") && !line.starts_with(' ') {
            let name = impl_target(trimmed);
            trait_impl = trimmed.contains(" for ");
            owner = Some(name.clone());
            if !trait_impl {
                push_impl(&mut items, &name);
            }
            continue;
        }
        if trimmed == "}" && !line.starts_with(' ') {
            owner = None;
            trait_impl = false;
            continue;
        }
        if trimmed == "#[macro_export]" {
            export_macro = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("macro_rules! ") {
            if export_macro {
                let name = rest.trim_end_matches(" {").trim_end_matches('{').trim();
                items.push(ApiItem {
                    kind: ItemKind::Decl,
                    text: format!("macro_rules! {name}"),
                    owner: None,
                });
            }
            export_macro = false;
            continue;
        }
        export_macro = false;
        let at_top = !line.starts_with(' ');
        if at_top && trimmed.starts_with("pub use ") {
            if trimmed.ends_with(';') {
                items.push(ApiItem {
                    kind: ItemKind::Decl,
                    text: tidy(trimmed),
                    owner: None,
                });
            } else {
                pending_use = trimmed.to_string();
            }
            continue;
        }
        if !trimmed.starts_with("pub ") {
            continue;
        }
        let body = trimmed.trim_start_matches("pub ");
        if owner.is_some()
            && (trait_impl || (!body.starts_with("fn ") && !body.starts_with("const ")))
        {
            continue;
        }
        if owner.is_none() && !ITEM_KEYWORDS.iter().any(|kw| body.starts_with(kw)) {
            continue;
        }
        if owner.is_none() && body.starts_with("mod ") && trimmed.ends_with(';') {
            continue;
        }
        items.push(ApiItem {
            kind: ItemKind::Decl,
            text: tidy(trimmed),
            owner: owner.clone(),
        });
    }
    items
}

fn push_impl(items: &mut Vec<ApiItem>, name: &str) {
    let text = format!("impl {name}");
    let duplicate = items
        .last()
        .is_some_and(|last| last.kind == ItemKind::Impl && last.text == text);
    if duplicate {
        return;
    }
    items.push(ApiItem {
        kind: ItemKind::Impl,
        text,
        owner: None,
    });
}

fn impl_target(header: &str) -> String {
    let rest = header.trim_start_matches("impl").trim();
    let rest = rest.trim_start_matches("<'a> ").trim();
    let head = rest.split('{').next().unwrap_or(rest).trim();
    let head = head.split(" where ").next().unwrap_or(head).trim();
    match head.split_once(" for ") {
        Some((_, target)) => target.trim().to_string(),
        None => head.to_string(),
    }
}

fn tidy(text: &str) -> String {
    let mut value = text.trim().to_string();
    let mut multi = false;
    for tail in [" {", " =>", ";"] {
        if let Some(stripped) = value.strip_suffix(tail) {
            value = stripped.trim_end().to_string();
        }
    }
    if value.ends_with('(') || value.ends_with(',') || value.ends_with("->") {
        multi = true;
    }
    let value = if multi {
        format!("{} …", value.trim_end_matches(',').trim_end())
    } else {
        value
    };
    collapse(&value)
}

fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(ch);
    }
    out
}

pub fn render_crate(api: &CrateApi) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} API 索引\n\n", api.name));
    out.push_str("> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。");
    out.push_str("完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。\n\n");
    for file in &api.files {
        out.push_str(&format!("## `{}`\n\n", file.path));
        for item in &file.items {
            match (&item.kind, &item.owner) {
                (ItemKind::Impl, _) => out.push_str(&format!("- `{}`\n", item.text)),
                (_, Some(_)) => out.push_str(&format!("  - `{}`\n", item.text)),
                _ => out.push_str(&format!("- `{}`\n", item.text)),
            }
        }
        out.push('\n');
    }
    out
}

pub fn render_index(crates: &[CrateApi]) -> String {
    let mut out = String::new();
    out.push_str("# API 索引\n\n");
    out.push_str(
        "按 crate 列出全部公开条目，由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。\n",
    );
    out.push_str(
        "需要参数类型、泛型约束或方法文档时，跑 `cargo doc --workspace --no-deps --open`。\n\n",
    );
    out.push_str("| crate | 条目 |\n|---|---|\n");
    for api in crates {
        out.push_str(&format!(
            "| [`{}`]({}.md) | {} |\n",
            api.name,
            api.name,
            api.item_count()
        ));
    }
    out.push_str("\n手写文档见 [../README.md](../README.md)。\n");
    out
}

pub fn write_all(root: &Path, crates: &[CrateApi]) -> Result<usize, String> {
    let dir = root.join(API_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建 {} 失败: {error}", dir.display()))?;
    let mut written = 0usize;
    for api in crates {
        let path = dir.join(format!("{}.md", api.name));
        let body = render_crate(api);
        fs::write(&path, body).map_err(|error| format!("写入 {} 失败: {error}", path.display()))?;
        written += 1;
    }
    let index = dir.join("README.md");
    fs::write(&index, render_index(crates))
        .map_err(|error| format!("写入 {} 失败: {error}", index.display()))?;
    Ok(written + 1)
}

pub fn check_all(root: &Path, crates: &[CrateApi]) -> Result<(), String> {
    let dir = root.join(API_DIR);
    let mut stale = Vec::new();
    for api in crates {
        let path = dir.join(format!("{}.md", api.name));
        let expected = render_crate(api);
        match fs::read_to_string(&path) {
            Ok(actual) if actual == expected => {}
            Ok(_) => stale.push(path),
            Err(_) => stale.push(path),
        }
    }
    let index = dir.join("README.md");
    match fs::read_to_string(&index) {
        Ok(actual) if actual == render_index(crates) => {}
        _ => stale.push(index),
    }
    if stale.is_empty() {
        return Ok(());
    }
    for path in &stale {
        let shown = path.strip_prefix(root).unwrap_or(path).display();
        println!("{shown} 与源码不一致");
    }
    Err(String::from(
        "API 索引已过期, 请运行 cargo run -p xtask -- gen-api",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_items_are_skipped() {
        let src = "fn hidden() {}\npub fn shown() {}\npub(crate) fn inner() {}\n";
        let items = extract(src);
        assert_eq!(items.len(), 1, "只保留真正的 pub 条目, 实际 {items:?}");
        assert_eq!(items[0].text, "pub fn shown() {}");
    }

    #[test]
    fn impl_methods_hang_under_their_type() {
        let src = "pub struct Builder;\nimpl Builder {\n    pub fn new() -> Self {\n        Self\n    }\n    fn private(&self) {}\n}\n";
        let items = extract(src);
        let parent: Vec<&str> = items
            .iter()
            .filter(|item| item.kind == ItemKind::Impl)
            .map(|item| item.text.as_str())
            .collect();
        assert_eq!(parent, vec!["impl Builder"], "impl 头应当只出现一次");
        let methods: Vec<&str> = items
            .iter()
            .filter(|item| item.owner.is_some())
            .map(|item| item.text.as_str())
            .collect();
        assert_eq!(methods, vec!["pub fn new() -> Self"], "只收录 pub 方法");
    }

    #[test]
    fn trait_impls_do_not_repeat_the_type() {
        let src = "pub struct Kit;\nimpl Default for Kit {\n    fn default() -> Self {\n        Self\n    }\n}\n";
        let items = extract(src);
        assert!(
            items.iter().all(|item| item.kind != ItemKind::Impl),
            "trait impl 不单独成行, 实际 {items:?}"
        );
    }

    #[test]
    fn multi_line_use_is_collapsed() {
        let src = "pub use kit::{\n    CanvasKit,\n    Color,\n};\n";
        let items = extract(src);
        assert_eq!(items.len(), 1);
        assert!(
            items[0].text.contains("CanvasKit, Color") || items[0].text.contains("CanvasKit,"),
            "多行 re-export 应当合并成一条, 实际 {}",
            items[0].text
        );
    }
}
