use std::path::{Path, PathBuf};
use std::sync::Arc;

use aurorix_render::{
    collection_face_count, font_container, font_face, font_family_bytes, font_family_count,
    font_family_names, has_font_family, normalize_family, register_font_family,
    skia_safe::surfaces, CanvasKit, FontContainer, PlaceholderBackend, RenderContext, TextStyle,
};

const PROBE_FAMILY: &str = "Aurorix Probe Sans";

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

fn preferred(fonts: &[PathBuf]) -> Option<&PathBuf> {
    const HINTS: [&str; 3] = ["dejavusans", "liberationsans", "notosans-regular"];
    let name_of = |path: &PathBuf| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    };
    fonts
        .iter()
        .find(|path| HINTS.iter().any(|hint| name_of(path).contains(hint)))
        .or_else(|| fonts.first())
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
    let chosen = preferred(&fonts)?;
    std::fs::read(chosen).ok()
}

#[test]
fn family_names_are_case_and_separator_insensitive() {
    assert_eq!(normalize_family("Exo-Medium"), "exomedium");
    assert_eq!(normalize_family("exo medium"), "exomedium");
    assert_eq!(normalize_family("exo_medium"), "exomedium");
    assert_eq!(normalize_family("EXOMEDIUM"), "exomedium");
    assert_eq!(normalize_family(" Exo \t Medium "), "exomedium");
    assert_eq!(normalize_family("---"), "");
}

#[test]
fn containers_are_recognised_by_signature() {
    assert_eq!(
        font_container(&[0, 1, 0, 0, 9, 9]),
        Some(FontContainer::TrueType)
    );
    assert_eq!(font_container(b"OTTOpayload"), Some(FontContainer::Cff));
    assert_eq!(
        font_container(b"ttcfpayload"),
        Some(FontContainer::Collection)
    );
    assert_eq!(font_container(b"wOFFpayload"), None);
    assert_eq!(font_container(&[0, 1]), None);
    assert_eq!(collection_face_count(b"ttcfpayload"), 1);
    assert_eq!(
        collection_face_count(b"ttcf\x00\x01\x00\x00\x00\x00\x00\x02"),
        2
    );
}

#[test]
fn invalid_bytes_and_blank_names_are_rejected() {
    let blank = register_font_family("   ", vec![0, 1, 0, 0], None);
    assert!(blank.is_err(), "空家族名必须报错");
    let junk = register_font_family("Aurorix Junk Face", b"not-a-font".to_vec(), None);
    assert!(junk.is_err(), "非法容器必须报错");
    let nul = register_font_family("Aurorix\0Nul", vec![0, 1, 0, 0], None);
    assert!(nul.is_err(), "含 NUL 的家族名必须报错");
    let bad_bold = register_font_family(
        "Aurorix Bold Junk",
        vec![0, 1, 0, 0],
        Some(b"junk".to_vec()),
    );
    assert!(bad_bold.is_err(), "粗体容器非法也要报错");
    assert!(!has_font_family("Aurorix Junk Face"));
}

#[test]
fn registered_family_resolves_without_a_system_manager() {
    let Some(bytes) = system_font() else {
        println!("跳过: 本机找不到可用 ttf 字体");
        return;
    };
    register_font_family(PROBE_FAMILY, bytes.clone(), None).expect("注册字体族");
    assert!(has_font_family(PROBE_FAMILY));
    assert!(has_font_family("aurorix_probe_sans"));
    assert!(has_font_family("AURORIXPROBESANS"));
    assert_eq!(
        font_face(PROBE_FAMILY).map(|face| face.regular.len()),
        Some(bytes.len())
    );
    assert_eq!(
        font_family_bytes("aurorix probe sans", false).map(|b| b.len()),
        Some(bytes.len())
    );
    assert_eq!(
        font_family_bytes(PROBE_FAMILY, true).map(|b| b.len()),
        Some(bytes.len())
    );

    let context = RenderContext::new(Arc::new(PlaceholderBackend::new()), 8, 1 << 20);
    assert!(context
        .fonts()
        .registered_families()
        .contains(&PROBE_FAMILY.to_string()));
    assert!(context.fonts().registered_family_count() >= 1);
    let resolved = context
        .fonts()
        .typeface("aurorix-probe-sans", false)
        .expect("已注册字体族应当可用");
    assert!(!resolved.family_name().is_empty(), "字形应当有族名");
    assert!(context.fonts().has_registered_family(PROBE_FAMILY));
}

#[test]
fn text_style_draws_with_registered_alias() {
    let Some(bytes) = system_font() else {
        println!("跳过: 本机找不到可用 ttf 字体");
        return;
    };
    register_font_family(PROBE_FAMILY, bytes, None).expect("注册字体族");
    let context = RenderContext::new(Arc::new(PlaceholderBackend::new()), 8, 1 << 20);
    let mut surface = surfaces::raster_n32_premul((160, 48)).expect("画布");
    {
        let kit = CanvasKit::new(surface.canvas(), &context);
        let style = TextStyle::new(PROBE_FAMILY, 32).with_bold(true);
        let metrics = kit.measure("Aurorix", &style).expect("可测量插件字体");
        assert!(metrics.width > 0.0, "插件字体应当给出宽度");
        kit.draw_text("Aurorix", 4.0, 34.0, &style)
            .expect("可绘制插件字体");
    }
    let image = surface.image_snapshot();
    let pixmap = image.peek_pixels().expect("像素可读");
    let painted = pixmap
        .bytes()
        .expect("像素字节")
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] > 0)
        .count();
    assert!(
        painted > 40,
        "插件字体必须真正画出字形, 实际 {painted} 个像素"
    );
    assert!(font_family_names().contains(&PROBE_FAMILY.to_string()));
    assert!(font_family_count() >= 1);
}
