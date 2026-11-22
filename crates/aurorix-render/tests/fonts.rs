use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aurorix_render::{
    collection_alias, collection_face_count, collection_face_index, font_container,
    font_family_bytes, font_family_names, has_font_family, measure_text, normalize_family,
    register_font_collection, register_font_family, FontContainer, PlaceholderBackend,
    RenderContext, RenderError, SharedFontMgr, SkiaBackend, TextStyle, COLLECTION_FACE_LIMIT,
};

const GUARD: Duration = Duration::from_secs(10);

const CJK_COLLECTION: &str = "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc";

const SINGLE_FACE_FONT: &str = "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf";

fn read_font(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn context() -> Arc<RenderContext> {
    RenderContext::new(Arc::new(PlaceholderBackend::new()), 8, 4096)
}

#[tokio::test]
async fn concurrent_load_of_same_font_calls_loader_once() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let calls = Arc::new(AtomicUsize::new(0));
        let handles = {
            let context = Arc::clone(&context);
            let calls = Arc::clone(&calls);
            tokio::task::spawn_blocking(move || {
                let mut threads = Vec::new();
                for _ in 0..8 {
                    let context = Arc::clone(&context);
                    let calls = Arc::clone(&calls);
                    threads.push(std::thread::spawn(move || {
                        context
                            .fonts()
                            .load("Inter", 16, move |family, size| {
                                calls.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(30));
                                Ok(Arc::new(vec![family.len() as u8, size as u8]))
                            })
                            .expect("字体加载应当成功")
                    }));
                }
                threads
                    .into_iter()
                    .map(|thread| thread.join().expect("字体线程不应崩溃"))
                    .collect::<Vec<_>>()
            })
            .await
            .expect("字体加载线程池不应崩溃")
        };

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "并发加载同一字体时 loader 只应被调用一次"
        );
        assert_eq!(handles.len(), 8, "八个调用都应当返回字体句柄");
        for handle in &handles {
            assert_eq!(&*handle.family, "Inter", "字体族名应当一致");
            assert_eq!(handle.size, 16, "字体尺寸应当一致");
        }
        assert_eq!(context.fonts().len(), 1, "字体簿中只应存在一个条目");
    })
    .await
    .expect("并发字体加载测试超时");
}

#[tokio::test]
async fn distinct_font_keys_load_separately() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        context
            .fonts()
            .load("Inter", 12, move |_family, _size| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(vec![12u8]))
            })
            .expect("首次加载应当成功");
        let counter = Arc::clone(&calls);
        context
            .fonts()
            .load("Inter", 16, move |_family, _size| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(vec![16u8]))
            })
            .expect("第二次加载应当成功");
        let counter = Arc::clone(&calls);
        context
            .fonts()
            .load("Inter", 12, move |_family, _size| {
                counter.fetch_add(10, Ordering::SeqCst);
                Ok(Arc::new(vec![0u8]))
            })
            .expect("重复加载应当命中缓存");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "不同字号各加载一次, 重复键不再触发 loader"
        );
        assert_eq!(context.fonts().len(), 2, "字体簿中应当有两个条目");
        let cached = context.fonts().get("Inter", 12).expect("缓存句柄应当可取");
        assert_eq!(cached.size, 12, "缓存句柄尺寸应当一致");
        assert!(
            context.fonts().get("Inter", 24).is_none(),
            "未加载的字号应当取不到句柄"
        );
    })
    .await
    .expect("字体键分离测试超时");
}

#[tokio::test]
async fn failed_font_load_is_not_cached() {
    tokio::time::timeout(GUARD, async {
        let context = context();
        let failure = context.fonts().load("Broken", 10, |family, _size| {
            Err(RenderError::Font(format!("找不到字体 {family}")))
        });
        match failure {
            Err(RenderError::Font(message)) => {
                assert!(message.contains("Broken"), "错误消息应当包含字体族名");
            }
            other => panic!("加载失败应当返回字体错误, 实际为 {other:?}"),
        }
        assert!(
            context.fonts().get("Broken", 10).is_none(),
            "加载失败的字体不应被缓存"
        );

        let retried = context
            .fonts()
            .load("Broken", 10, |_family, _size| Ok(Arc::new(vec![1u8, 2u8])))
            .expect("重试加载应当成功");
        assert_eq!(retried.size, 10, "重试句柄尺寸应当一致");
        assert!(
            context.fonts().get("Broken", 10).is_some(),
            "重试成功后应当可以取到句柄"
        );
    })
    .await
    .expect("字体失败缓存测试超时");
}

#[tokio::test]
async fn font_capacity_evicts_oldest_entry() {
    tokio::time::timeout(GUARD, async {
        let context = RenderContext::new(Arc::new(PlaceholderBackend::new()), 2, 4096);
        for size in [10u32, 12, 14] {
            context
                .fonts()
                .load("Inter", size, move |_family, size| {
                    Ok(Arc::new(vec![size as u8]))
                })
                .expect("字体加载应当成功");
        }
        assert_eq!(
            context.fonts().len(),
            2,
            "容量上限为 2 时应当只保留两个条目"
        );
        assert!(
            context.fonts().get("Inter", 10).is_none(),
            "最旧的字号条目应当被淘汰"
        );
        assert!(
            context.fonts().get("Inter", 14).is_some(),
            "最新的字号条目应当保留"
        );
    })
    .await
    .expect("字体容量测试超时");
}

#[tokio::test]
async fn real_font_manager_loads_single_flight() {
    tokio::time::timeout(GUARD, async {
        let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 4096);
        let calls = Arc::new(AtomicUsize::new(0));
        let handles = {
            let context = Arc::clone(&context);
            let calls = Arc::clone(&calls);
            tokio::task::spawn_blocking(move || {
                let mut threads = Vec::new();
                for _ in 0..8 {
                    let context = Arc::clone(&context);
                    let calls = Arc::clone(&calls);
                    threads.push(std::thread::spawn(move || {
                        context
                            .fonts()
                            .load("sans-serif", 24, move |_family, _size| {
                                calls.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(20));
                                Ok(Arc::new(Vec::new()))
                            })
                            .expect("字体加载应当成功")
                    }));
                }
                threads
                    .into_iter()
                    .map(|thread| thread.join().expect("字体线程不应崩溃"))
                    .collect::<Vec<_>>()
            })
            .await
            .expect("字体加载线程池不应崩溃")
        };
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "并发加载同一字体时 loader 只应被调用一次"
        );
        assert_eq!(handles.len(), 8, "八个调用都应当返回字体句柄");
        assert!(
            context.fonts().get("sans-serif", 24).is_some(),
            "真实字体管理器下加载后应当可以取到句柄"
        );
    })
    .await
    .expect("真实字体单飞测试超时");
}

#[tokio::test]
async fn unknown_family_falls_back_to_default_typeface() {
    tokio::time::timeout(GUARD, async {
        let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 4096);
        assert!(
            context.fonts().family_count() > 0,
            "系统字体管理器应当能枚举字体族"
        );
        let resolved = context
            .fonts()
            .typeface("Aurorix-NoSuchFamily", false)
            .expect("未知字体族应当回退到默认字形");
        assert!(!resolved.family_name().is_empty(), "回退字形应当有族名");
        let bold = context
            .fonts()
            .typeface("Aurorix-NoSuchFamily", true)
            .expect("粗体未知字体族应当回退到默认字形");
        assert!(!bold.family_name().is_empty(), "粗体回退字形应当有族名");
        let chain = context
            .fonts()
            .fallback_chain("Aurorix-NoSuchFamily", false);
        assert!(!chain.is_empty(), "回退链不应当为空");
        let default_name = resolved.family_name();
        let deduped = context.fonts().fallback_chain(&default_name, false);
        assert_eq!(deduped.len(), 1, "回退链应当对同名字形去重");
        let other = context.fonts().fallback_chain("sans-serif", false);
        assert!(!other.is_empty(), "已知字体族的回退链不应当为空");
    })
    .await
    .expect("字体回退测试超时");
}

#[test]
fn container_contract_is_stable() {
    use aurorix_render::FontContainer;
    assert_eq!(FontContainer::TrueType.label(), "truetype");
    assert_eq!(FontContainer::Cff.label(), "opentype-cff");
    assert_eq!(FontContainer::Collection.label(), "ttc");
    assert!(!FontContainer::TrueType.is_collection());
    assert!(!FontContainer::Cff.is_collection());
    assert!(FontContainer::Collection.is_collection());
    assert_eq!(
        FontContainer::TrueType.signature(),
        [0x00, 0x01, 0x00, 0x00]
    );
    assert_eq!(FontContainer::Cff.signature(), *b"OTTO");
    assert_eq!(FontContainer::Collection.signature(), *b"ttcf");
}

#[test]
fn collection_alias_and_index_roundtrip() {
    assert_eq!(collection_alias("Noto Sans CJK SC", 1), "notosanscjksc#1");
    assert_eq!(collection_alias("Noto-Sans_CJK\tSC", 3), "notosanscjksc#3");
    assert_eq!(collection_face_index("notosanscjksc#1"), 1);
    assert_eq!(
        collection_face_index(&collection_alias("Noto Sans CJK SC", 4)),
        4
    );
    assert_eq!(collection_face_index("Noto Sans CJK SC"), 0);
    assert_eq!(collection_face_index("#3"), 0);
    assert_eq!(collection_face_index(""), 0);
    assert_eq!(collection_face_index("notosanscjksc#x"), 0);
    assert_eq!(normalize_family("notosanscjksc#2"), "notosanscjksc#2");
}

#[test]
fn collection_face_count_reads_header_or_single_face() {
    assert_eq!(collection_face_count(b"not-a-font"), 1);
    assert_eq!(collection_face_count(&[0x00, 0x01, 0x00, 0x00, 9, 9]), 1);
    assert_eq!(collection_face_count(b"ttcf"), 1);
    assert_eq!(
        collection_face_count(b"ttcf\x00\x01\x00\x00\x00\x00\x00\x03"),
        3
    );
    assert_eq!(
        collection_face_count(b"ttcf\x00\x01\x00\x00\x00\x00\x40\x00"),
        COLLECTION_FACE_LIMIT
    );
}

#[test]
fn collection_font_exposes_every_face() {
    let Some(bytes) = read_font(CJK_COLLECTION) else {
        return;
    };
    assert_eq!(font_container(&bytes), Some(FontContainer::Collection));
    let faces = SharedFontMgr::system().typeface_from_collection(&bytes);
    assert!(
        faces.len() > 1,
        "多字重 TTC 应当枚举出多个 face, 实际 {}",
        faces.len()
    );
    assert!(faces.len() <= COLLECTION_FACE_LIMIT, "枚举数量不应超过上限");
    assert_eq!(
        collection_face_count(&bytes),
        faces.len(),
        "TTC 头部 face 数应当与枚举结果一致"
    );
    assert!(
        faces.iter().all(|face| !face.family_name().is_empty()),
        "每个 face 都应当有族名"
    );
}

#[test]
fn register_font_collection_registers_alias_faces() {
    let Some(bytes) = read_font(CJK_COLLECTION) else {
        return;
    };
    let family = "Aurorix CJK Collection Probe";
    let registered = register_font_collection(family, &bytes);
    assert!(registered > 1, "集合应当注册多个 face, 实际 {registered}");
    assert!(has_font_family(family), "原始家族名应当可查到");
    let alias = collection_alias(family, 1);
    assert_eq!(alias, "aurorixcjkcollectionprobe#1");
    assert!(has_font_family(&alias), "#1 别名应当可查到");
    assert!(
        font_family_names().contains(&alias),
        "font_family_names 应当能观察到 #1 别名"
    );
    let alias_bytes = font_family_bytes(&alias, false).expect("别名应当能取到字节");
    assert_eq!(alias_bytes.len(), bytes.len(), "别名应当复用集合字节");
    assert_eq!(
        register_font_collection(family, &bytes),
        registered,
        "重复注册的 face 数应当稳定"
    );
    assert!(
        !has_font_family(&collection_alias(family, registered)),
        "超出 face 总数的别名不应存在"
    );
}

#[test]
fn register_font_family_expands_collections() {
    let Some(bytes) = read_font(CJK_COLLECTION) else {
        return;
    };
    let family = "Aurorix Family Collection Probe";
    register_font_family(family, bytes.clone(), None).expect("注册 TTC 家族");
    assert!(has_font_family(family), "原始家族名应当可查到");
    let alias = collection_alias(family, 1);
    assert!(
        has_font_family(&alias),
        "register_font_family 也应当展开 #1 别名"
    );
    assert_eq!(
        font_family_bytes(&alias, false).map(|data| data.len()),
        Some(bytes.len()),
        "别名应当取到集合字节"
    );
}

#[test]
fn font_book_resolves_collection_alias_face() {
    let Some(bytes) = read_font(CJK_COLLECTION) else {
        return;
    };
    let family = "Aurorix Alias Lookup Probe";
    let registered = register_font_collection(family, &bytes);
    let faces = SharedFontMgr::system().typeface_from_collection(&bytes);
    assert_eq!(registered, faces.len(), "注册 face 数应当与枚举一致");
    let context = RenderContext::new(Arc::new(SkiaBackend::new()), 8, 4096);
    let first = context
        .fonts()
        .typeface(family, false)
        .expect("原始家族名应当可解析");
    assert_eq!(
        first.family_name(),
        faces[0].family_name(),
        "原名应当对应 index 0"
    );
    let alias = collection_alias(family, 1);
    let second = context
        .fonts()
        .typeface(&alias, false)
        .expect("#1 别名应当可解析");
    assert_eq!(
        second.family_name(),
        faces[1].family_name(),
        "#1 别名应当对应 index 1"
    );
    let style = TextStyle::new(alias, 24);
    let width = measure_text(context.fonts(), "字体测试", &style).expect("按别名测量应当成功");
    assert!(width > 0.0, "按别名测量出的宽度应当为正, 实际 {width}");
}

#[test]
fn single_face_font_registers_exactly_once() {
    let Some(bytes) = read_font(SINGLE_FACE_FONT) else {
        return;
    };
    assert!(
        !font_container(&bytes)
            .map(|container| container.is_collection())
            .unwrap_or(false),
        "单字重字体不应被识别为集合"
    );
    assert_eq!(collection_face_count(&bytes), 1, "单字重字体的 face 数为 1");
    let family = "Aurorix Adwaita Probe";
    assert_eq!(
        register_font_collection(family, &bytes),
        1,
        "非集合字体应当只注册一个家族"
    );
    assert!(has_font_family(family), "家族名应当可查到");
    assert!(
        !has_font_family(&collection_alias(family, 1)),
        "非集合字体不应生成别名"
    );
    assert_eq!(register_font_collection("Aurorix Empty Probe", b""), 0);
    assert_eq!(register_font_collection("   ", &bytes), 0);
}
