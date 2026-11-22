use aurorix_render::{
    clear_font_families, font_family_count, has_font_family, register_font_family,
};

#[test]
fn clear_drops_every_registered_family() {
    register_font_family("Aurorix Reset Face", vec![0, 1, 0, 0, 1, 2, 3, 4], None)
        .expect("注册字体族");
    assert!(has_font_family("aurorix-reset-face"));
    assert!(font_family_count() >= 1);
    clear_font_families();
    assert!(!has_font_family("Aurorix Reset Face"));
    assert_eq!(font_family_count(), 0);
}
