use std::sync::Arc;

use aurorix_core::MemoryPressure;
use aurorix_render::cache::{content_key, TextureCache};
use aurorix_render::codec::{encode_rgba, EncodeOptions};
use aurorix_render::draw::RenderFormat;
use aurorix_render::memory::{
    active_renders, enter_render, purge_textures_if_idle, register_texture_cache,
};

fn case_001() -> bytes::Bytes {
    encode_rgba(
        RenderFormat::Png,
        &[200u8, 30, 40, 255].repeat(64),
        8,
        8,
        32,
        &EncodeOptions::new(80),
    )
    .expect("测试图编码应当成功")
}

#[test]
fn case_002() {
    let cache = Arc::new(TextureCache::new(4 * 1024 * 1024));
    register_texture_cache(&cache);
    assert_eq!(active_renders(), 0, "空闲时活跃渲染计数必须为 0");

    let bytes = case_001();
    let key = content_key(&bytes);
    let texture = cache.image_for(bytes.clone()).expect("纹理应当可获取");
    assert!(texture.image().is_some(), "首次取图应当解码出位图");

    let guard = enter_render();
    let dropped = purge_textures_if_idle(MemoryPressure::Normal);
    let kept = cache.get(&key).expect("编码字节不能被回收掉");
    assert_eq!(dropped, 0, "并发渲染进行中不得清除已解码位图");
    assert!(kept.image().is_some(), "并发渲染进行中位图必须原样保留");

    let dropped = purge_textures_if_idle(MemoryPressure::High);
    let forced = cache.get(&key).expect("编码字节不能被回收掉");
    assert_eq!(dropped, 1, "内存高压必须无视活跃渲染直接回收");
    assert!(forced.image().is_none(), "高压回收后位图应当被释放");
    drop(guard);

    let _ = cache.image_for(bytes).expect("纹理应当可获取");
    let dropped = purge_textures_if_idle(MemoryPressure::Normal);
    let after = cache.get(&key).expect("编码字节不能被回收掉");
    assert_eq!(dropped, 1, "空闲回收必须清掉一份已解码位图");
    assert!(after.image().is_none(), "空闲回收后位图应当被释放");
    assert_eq!(cache.stats().entries, 1, "回收位图不能丢掉编码字节");
}
