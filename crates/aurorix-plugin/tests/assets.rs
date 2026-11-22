use std::path::{Path, PathBuf};
use std::sync::Arc;

use aurorix_core::{Scene, Target};
use aurorix_plugin::{AssetPathError, Incoming, Invocation, PluginAssets};
use aurorix_render::RenderError;
use aurorix_template::{Registry, TemplateKind};

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "aurorix-plugin-assets-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("创建临时目录");
        Self { root }
    }

    fn file(&self, relative: &str, data: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("创建父目录");
        }
        std::fs::write(&path, data).expect("写文件");
        path
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn collect_fonts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, depth + 1, out);
        } else if path
            .extension()
            .map(|extension| extension.eq_ignore_ascii_case("ttf"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

fn system_font() -> Option<Vec<u8>> {
    let roots = [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/System/Library/Fonts",
        "C:/Windows/Fonts",
    ];
    let mut fonts = Vec::new();
    for root in roots {
        collect_fonts(Path::new(root), 0, &mut fonts);
    }
    fonts.sort();
    fonts.iter().find_map(|path| std::fs::read(path).ok())
}

fn collect_suffix(dir: &Path, suffix: &str, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_suffix(&path, suffix, depth + 1, out);
        } else if path
            .extension()
            .map(|extension| extension.eq_ignore_ascii_case(suffix))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

fn system_collection() -> Option<Vec<u8>> {
    let roots = [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/System/Library/Fonts",
        "C:/Windows/Fonts",
    ];
    let mut fonts = Vec::new();
    for root in roots {
        collect_suffix(Path::new(root), "ttc", 0, &mut fonts);
    }
    fonts.sort();
    fonts.iter().find_map(|path| std::fs::read(path).ok())
}

#[test]
fn unsafe_keys_are_rejected() {
    let tree = TempTree::new("escape");
    tree.file("data/hero.bin", b"hero");
    let assets = PluginAssets::new("rating", tree.path());
    assert_eq!(assets.plugin_id(), "rating");
    assert_eq!(assets.root(), tree.root.canonicalize().expect("根目录"));
    assert!(assets.resolve("data/hero.bin").is_ok());
    assert!(assets.exists("data/hero.bin"));
    assert!(!assets.exists("data/none.bin"));

    let escape = |key: &str| assets.resolve(key).expect_err(key);
    assert!(matches!(escape("../escape.txt"), AssetPathError::Escape(_)));
    assert!(matches!(
        escape("data/../../escape.txt"),
        AssetPathError::Escape(_)
    ));
    assert!(matches!(
        escape("data\\hero.bin"),
        AssetPathError::Escape(_)
    ));
    assert!(matches!(escape(""), AssetPathError::Escape(_)));
    assert!(matches!(escape("   "), AssetPathError::Escape(_)));
    assert!(matches!(escape("./"), AssetPathError::Escape(_)));
    assert!(matches!(escape("/etc/passwd"), AssetPathError::Absolute(_)));
    assert!(matches!(
        escape("C:/Windows/win.ini"),
        AssetPathError::Absolute(_)
    ));
    assert!(matches!(
        assets.resolve("data/missing.bin"),
        Err(AssetPathError::Missing(_))
    ));
    assert!(matches!(
        assets.resolve("data"),
        Err(AssetPathError::Missing(_))
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_that_leave_the_root_are_rejected() {
    let tree = TempTree::new("symlink");
    let outside = TempTree::new("symlink-outside");
    let target = outside.file("secret.bin", b"secret");
    tree.file("data/hero.bin", b"hero");
    std::os::unix::fs::symlink(&target, tree.root.join("leak.bin")).expect("建立符号链接");
    std::os::unix::fs::symlink(&outside.root, tree.root.join("leakdir")).expect("建立目录链接");
    let assets = PluginAssets::new("rating", tree.path());
    assert!(matches!(
        assets.resolve("leak.bin"),
        Err(AssetPathError::Escape(_))
    ));
    assert!(matches!(
        assets.read("leakdir/secret.bin"),
        Err(AssetPathError::Escape(_))
    ));
    assert!(assets.resolve("data/hero.bin").is_ok());
}

#[test]
fn read_enforces_the_size_budget() {
    let tree = TempTree::new("budget");
    tree.file("data/hero.bin", b"0123456789");
    let assets = PluginAssets::new("rating", tree.path());
    assert_eq!(assets.read("data/hero.bin").expect("读回素材").len(), 10);
    assert_eq!(assets.max_bytes(), 32 * 1024 * 1024);
    let small = PluginAssets::new("rating", tree.path()).with_max_bytes(4);
    match small.read("data/hero.bin") {
        Err(AssetPathError::TooLarge { key, size, limit }) => {
            assert_eq!(key, "data/hero.bin");
            assert_eq!(size, 10);
            assert_eq!(limit, 4);
        }
        other => panic!("应当报告超限: {other:?}"),
    }
    assert!(matches!(
        assets.read("data/none.bin"),
        Err(AssetPathError::Missing(_))
    ));
}

#[test]
fn fonts_and_templates_are_enumerated() {
    let tree = TempTree::new("listing");
    tree.file("fonts/Exo-Medium.ttf", b"x");
    tree.file("fonts/星尘.otf", b"x");
    tree.file("fonts/ignored.txt", b"x");
    tree.file("templates/card.md", b"card");
    tree.file("templates/notes.txt", b"x");
    let assets = PluginAssets::new("rating", tree.path());
    assert_eq!(
        assets.fonts(),
        vec!["Exo-Medium".to_string(), "星尘".to_string()]
    );
    assert_eq!(assets.templates(), vec!["card".to_string()]);
    assert_eq!(
        assets.font_alias("exo_medium"),
        Some("Exo-Medium".to_string())
    );
    assert_eq!(
        assets.font_alias("EXO MEDIUM.ttf"),
        Some("Exo-Medium".to_string())
    );
    assert_eq!(
        assets.font_alias("fonts/Exo-Medium.ttf"),
        Some("Exo-Medium".to_string())
    );
    assert_eq!(assets.font_alias("星尘.otf"), Some("星尘".to_string()));
    assert_eq!(assets.font_alias("nope"), None);
}

#[test]
fn bootstrap_registers_templates_and_reports_skips() {
    let tree = TempTree::new("bootstrap");
    tree.file("templates/card.md", b"card {{v}}");
    tree.file("fonts/Broken.ttf", b"not-a-font");
    tree.file("fonts/Collection.ttc", b"ttcf-not-really");
    let real_font = system_font();
    if let Some(bytes) = real_font.as_ref() {
        tree.file("fonts/Aurorix Assets Sans.ttf", bytes);
    }
    let assets = PluginAssets::new("rating", tree.path());
    let registry = Arc::new(Registry::new());
    let report = assets.bootstrap(&registry);
    assert!(report.root_ready, "根目录存在");
    assert_eq!(report.templates, vec!["card".to_string()]);
    assert!(report.registered() >= 2);
    let broken = report
        .skipped
        .iter()
        .find(|(key, _)| key == "fonts/Broken.ttf")
        .expect("坏字体要进 skipped");
    assert!(
        broken.1.contains("TrueType") || broken.1.contains("容器"),
        "{:?}",
        broken.1
    );
    let collection = report
        .skipped
        .iter()
        .find(|(key, _)| key == "fonts/Collection.ttc")
        .expect("坏 TTC 要进 skipped");
    assert!(collection.1.contains("TTC"), "{}", collection.1);
    if real_font.is_some() {
        assert!(report.fonts.contains(&"Aurorix Assets Sans".to_string()));
    }
    if let Some(bytes) = system_collection() {
        let tree = TempTree::new("bootstrap-collection");
        tree.file("fonts/Aurorix Faces.ttc", &bytes);
        let assets = PluginAssets::new("rating", tree.path());
        let report = assets.bootstrap(&Arc::new(Registry::new()));
        assert!(report.fonts.contains(&"Aurorix Faces".to_string()));
        assert!(
            report.fonts.iter().any(|face| face == "aurorixfaces#1"),
            "TTC 的第二个 face 也要注册: {:?}",
            report.fonts
        );
    }
    let args = aurorix_template::args_map! { "v" => "X" };
    assert_eq!(
        registry
            .fill(TemplateKind::Markdown, "rating", "card", &args)
            .expect("插件命名空间下的模板"),
        "card X"
    );
}

#[test]
fn missing_root_reports_not_ready() {
    let tree = TempTree::new("missing-root");
    let assets = PluginAssets::new("rating", tree.root.join("nope"));
    let registry = Arc::new(Registry::new());
    let report = assets.bootstrap(&registry);
    assert!(!report.root_ready);
    assert!(report.fonts.is_empty());
    assert!(report.templates.is_empty());
    assert!(!assets.root_ready());
    assert!(matches!(
        assets.read("data/hero.bin"),
        Err(AssetPathError::Missing(_))
    ));
}

#[test]
fn loader_reads_relative_keys_through_the_asset_trait() {
    let tree = TempTree::new("loader");
    tree.file("data/hero.bin", b"pixels");
    let assets = Arc::new(PluginAssets::new("rating", tree.path()));
    let loader = assets.loader();
    assert_eq!(
        loader.load("data/hero.bin").expect("读素材").as_ref(),
        b"pixels"
    );
    let error = loader.load("../secret").expect_err("越界必须报错");
    assert!(matches!(error, RenderError::Texture(_)), "{error:?}");
}

#[test]
fn invocation_carries_optional_plugin_assets() {
    let tree = TempTree::new("invocation");
    let assets = Arc::new(PluginAssets::new("rating", tree.path()));
    let event = Incoming::new(Scene::Group, Target::new(Scene::Group, "g1"), "u1", "/b30")
        .with_username("测试用户");
    let bare = Invocation::from_event(&event);
    assert!(!bare.has_plugin_assets());
    assert!(bare.assets().is_none());
    assert!(bare.opt_plugin_assets().is_none());
    let wired = Invocation::from_event(&event).with_plugin_assets(Arc::clone(&assets));
    assert!(wired.has_plugin_assets());
    assert_eq!(
        wired.assets().map(|value| value.plugin_id()),
        Some("rating")
    );
    assert_eq!(
        wired
            .opt_plugin_assets()
            .map(|value| value.plugin_id().to_string()),
        Some("rating".to_string())
    );
    assert_ne!(bare, wired);
    assert_eq!(wired.clone(), wired);
}

#[test]
fn reload_driver_needs_a_registry_and_a_root() {
    let tree = TempTree::new("reload-none");
    let assets = Arc::new(PluginAssets::new("rating", tree.path()));
    assert!(assets.registry().is_none());
    assert!(assets.spawn_template_reload(tree.path()).is_none());
    let registry = Arc::new(Registry::new());
    let _ = assets.bootstrap(&registry);
    assert!(assets.registry().is_some());
    assert!(assets.spawn_template_reload("").is_none());
}

#[tokio::test]
async fn reload_driver_applies_namespace_overrides() {
    let tree = TempTree::new("reload");
    tree.file("templates/card.md", b"base {{v}}");
    let assets = Arc::new(PluginAssets::new("rating", tree.path()));
    let registry = Arc::new(Registry::new());
    assets.bootstrap(&registry);
    let overrides = TempTree::new("reload-overrides");
    overrides.file("rating/card.md", b"override {{v}}");
    let mut handle = assets
        .spawn_template_reload(overrides.path())
        .expect("应当启动驱动器");
    assert!(handle.is_running(), "驱动器应当在运行");
    let args = aurorix_template::args_map! { "v" => "X" };
    assert_eq!(
        registry
            .fill(TemplateKind::Markdown, "rating", "card", &args)
            .expect("覆盖后的模板"),
        "override X"
    );
    handle.stop();
    assert!(!handle.is_running());
}

#[test]
fn report_is_empty_only_when_nothing_happened() {
    use aurorix_plugin::PluginAssetsReport;
    let report = PluginAssetsReport::default();
    assert!(report.is_empty(), "全新的报告应当是空的");
    assert_eq!(report.registered(), 0);

    let mut fonts = PluginAssetsReport::default();
    fonts.fonts.push("Exo-Medium".to_string());
    assert!(!fonts.is_empty(), "注册了字体就不算空");
    assert_eq!(fonts.registered(), 1);

    let mut templates = PluginAssetsReport::default();
    templates.templates.push("Card".to_string());
    assert!(!templates.is_empty(), "注册了模板就不算空");
    assert_eq!(templates.registered(), 1);

    let mut skipped = PluginAssetsReport::default();
    skipped
        .skipped
        .push(("broken.ttf".to_string(), "坏字体".to_string()));
    assert!(!skipped.is_empty(), "有跳过项就不算空");
    assert_eq!(skipped.registered(), 0);

    let missing_root = PluginAssetsReport {
        root_ready: true,
        ..PluginAssetsReport::default()
    };
    assert!(!missing_root.is_empty(), "资源目录就绪就不算空");
}
