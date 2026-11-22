use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Once, Weak};

use aurorix_core::{check_memory_pressure, HardwareProfile, HardwareTier, MemoryPressure};
use parking_lot::Mutex;

use crate::batch::TextBlobCache;
use crate::cache::TextureCache;

pub const PURGE_EVERY: u64 = 8;

pub const RECLAIM_EVERY: u64 = 64;

static RENDERS: AtomicU64 = AtomicU64::new(0);

static ACTIVE_RENDERS: AtomicUsize = AtomicUsize::new(0);

static CONFIGURED: Once = Once::new();

static SURFACE_EPOCH: AtomicU64 = AtomicU64::new(0);

static TEXTURES: Mutex<Vec<Weak<TextureCache>>> = Mutex::new(Vec::new());

static TEXT_BLOBS: Mutex<Vec<Weak<TextBlobCache>>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReclaimReport {
    pub textures: usize,
    pub text_blobs: usize,
    pub trimmed: bool,
}

impl ReclaimReport {
    pub fn total(self) -> usize {
        self.textures + self.text_blobs
    }
}

pub fn configure_caches() {
    CONFIGURED.call_once(|| {
        let profile = HardwareProfile::cached();
        skia_safe::graphics::set_font_cache_limit(profile.skia_font_cache_bytes());
        skia_safe::graphics::set_resource_cache_total_bytes_limit(
            profile.skia_resource_cache_bytes(),
        );
    });
}

pub fn register_texture_cache(cache: &Arc<TextureCache>) {
    let mut caches = TEXTURES.lock();
    caches.retain(|cached| cached.strong_count() > 0);
    if caches
        .iter()
        .any(|cached| cached.ptr_eq(&Arc::downgrade(cache)))
    {
        return;
    }
    caches.push(Arc::downgrade(cache));
}

pub fn register_text_blob_cache(cache: &Arc<TextBlobCache>) {
    let mut caches = TEXT_BLOBS.lock();
    caches.retain(|cached| cached.strong_count() > 0);
    if caches
        .iter()
        .any(|cached| cached.ptr_eq(&Arc::downgrade(cache)))
    {
        return;
    }
    caches.push(Arc::downgrade(cache));
}

pub fn purge_textures() -> usize {
    let caches: Vec<Arc<TextureCache>> = {
        let mut registered = TEXTURES.lock();
        registered.retain(|cached| cached.strong_count() > 0);
        registered.iter().filter_map(Weak::upgrade).collect()
    };
    caches.iter().map(|cache| cache.drop_decoded()).sum()
}

pub fn purge_text_blobs() -> usize {
    let caches: Vec<Arc<TextBlobCache>> = {
        let mut registered = TEXT_BLOBS.lock();
        registered.retain(|cached| cached.strong_count() > 0);
        registered.iter().filter_map(Weak::upgrade).collect()
    };
    let mut cleared = 0;
    for cache in caches {
        cleared += cache.len();
        cache.clear();
    }
    cleared
}

pub struct ActiveRender {
    _private: (),
}

impl Drop for ActiveRender {
    fn drop(&mut self) {
        ACTIVE_RENDERS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn enter_render() -> ActiveRender {
    ACTIVE_RENDERS.fetch_add(1, Ordering::SeqCst);
    ActiveRender { _private: () }
}

pub fn active_renders() -> usize {
    ACTIVE_RENDERS.load(Ordering::SeqCst)
}

pub fn should_purge_textures(active: usize, pressure: MemoryPressure) -> bool {
    active == 0 || pressure.is_high()
}

pub fn purge_textures_if_idle(pressure: MemoryPressure) -> usize {
    if should_purge_textures(active_renders(), pressure) {
        purge_textures()
    } else {
        0
    }
}

pub fn reclaim() -> usize {
    purge_caches();
    purge_textures_if_idle(check_memory_pressure())
}

pub fn purge_caches() {
    skia_safe::graphics::purge_all_caches();
}

pub fn trim_memory() -> bool {
    aurorix_core::trim_memory()
}

pub fn pressure() -> MemoryPressure {
    check_memory_pressure()
}

pub fn emergency_reclaim() -> ReclaimReport {
    let textures = purge_textures_if_idle(check_memory_pressure());
    let text_blobs = purge_text_blobs();
    purge_caches();
    bump_surface_epoch();
    let trimmed = trim_memory();
    ReclaimReport {
        textures,
        text_blobs,
        trimmed,
    }
}

pub fn before_render() -> MemoryPressure {
    let pressure = check_memory_pressure();
    if pressure.is_high() {
        let _ = emergency_reclaim();
    }
    pressure
}

pub fn after_render() {
    let done = RENDERS.fetch_add(1, Ordering::SeqCst) + 1;
    if HardwareProfile::cached().tier() == HardwareTier::Nano {
        let _ = emergency_reclaim();
        return;
    }
    if check_memory_pressure().is_high() {
        let _ = emergency_reclaim();
        return;
    }
    if done.is_multiple_of(RECLAIM_EVERY) {
        reclaim();
        let _ = trim_memory();
        return;
    }
    if !done.is_multiple_of(PURGE_EVERY) {
        return;
    }
    purge_caches();
    let _ = trim_memory();
}

pub fn renders() -> u64 {
    RENDERS.load(Ordering::SeqCst)
}

pub fn surface_epoch() -> u64 {
    SURFACE_EPOCH.load(Ordering::SeqCst)
}

pub fn bump_surface_epoch() -> u64 {
    SURFACE_EPOCH.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn surface_cache_limit() -> usize {
    HardwareProfile::cached().surface_cache()
}

pub fn font_cache_limit() -> usize {
    HardwareProfile::cached().skia_font_cache_bytes()
}

pub fn resource_cache_limit() -> usize {
    HardwareProfile::cached().skia_resource_cache_bytes()
}

pub fn text_blob_capacity() -> usize {
    match HardwareProfile::cached().tier() {
        HardwareTier::Nano => 128,
        HardwareTier::Compact => 256,
        HardwareTier::Balanced => 1024,
        HardwareTier::Performance => 4096,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_limits_follow_hardware_profile() {
        configure_caches();
        let profile = HardwareProfile::cached();
        assert_eq!(font_cache_limit(), profile.skia_font_cache_bytes());
        assert_eq!(resource_cache_limit(), profile.skia_resource_cache_bytes());
        assert_eq!(surface_cache_limit(), profile.surface_cache());
        assert!(text_blob_capacity() >= 128, "排版缓存容量必须为正");
    }

    #[test]
    fn every_render_batch_triggers_a_purge() {
        let before = renders();
        for _ in 0..PURGE_EVERY {
            after_render();
        }
        assert!(
            renders() >= before + PURGE_EVERY,
            "渲染计数要随每次渲染递增"
        );
    }

    #[test]
    fn registered_texture_caches_are_reclaimed() {
        let cache = Arc::new(TextureCache::new(4 * 1024 * 1024));
        register_texture_cache(&cache);
        let bytes = crate::codec::encode_rgba(
            crate::draw::RenderFormat::Png,
            &[255u8, 0, 0, 255].repeat(64),
            8,
            8,
            32,
            &crate::codec::EncodeOptions::new(80),
        )
        .expect("测试图编码应当成功");
        let key = crate::cache::content_key(&bytes);
        let texture = cache.image_for(bytes).expect("纹理应当可获取");
        assert!(texture.image().is_some(), "首次取图应当解码出位图");
        assert_eq!(cache.decodes(), 1, "首次取图应当解码一次");
        purge_textures();
        let kept = cache.get(&key).expect("编码字节不能被回收掉");
        assert!(kept.image().is_none(), "注册过的纹理缓存必须被回收位图");
        assert_eq!(cache.stats().entries, 1, "回收位图不能丢掉编码字节");
    }

    #[test]
    fn emergency_reclaim_clears_every_cache() {
        let textures = Arc::new(TextureCache::new(4 * 1024 * 1024));
        register_texture_cache(&textures);
        let blobs = Arc::new(TextBlobCache::new(16));
        register_text_blob_cache(&blobs);
        let bytes = crate::codec::encode_rgba(
            crate::draw::RenderFormat::Png,
            &[12u8, 34, 56, 255].repeat(64),
            8,
            8,
            32,
            &crate::codec::EncodeOptions::new(80),
        )
        .expect("测试图编码应当成功");
        let key = crate::cache::content_key(&bytes);
        let texture = textures.image_for(bytes).expect("纹理应当可获取");
        assert!(texture.image().is_some(), "首次取图应当解码出位图");
        let epoch = surface_epoch();
        let _ = emergency_reclaim();
        let kept = textures.get(&key).expect("编码字节仍需保留");
        assert!(kept.image().is_none(), "紧急回收必须清掉已解码位图");
        assert!(
            surface_epoch() > epoch,
            "紧急回收必须让表面池换一代, 迫使各线程清空画布"
        );
        assert_eq!(textures.stats().entries, 1, "编码字节仍需保留");
        assert_eq!(blobs.len(), 0, "排版缓存必须清空");
    }

    #[test]
    fn before_render_reports_pressure_without_panicking() {
        let pressure = before_render();
        assert!(
            pressure == MemoryPressure::Normal || pressure == MemoryPressure::High,
            "压力状态只能是正常或高压"
        );
    }

    #[test]
    fn should_purge_textures_only_when_idle_or_high_pressure() {
        assert!(should_purge_textures(0, MemoryPressure::Normal));
        assert!(!should_purge_textures(1, MemoryPressure::Normal));
        assert!(should_purge_textures(1, MemoryPressure::High));
        assert!(should_purge_textures(0, MemoryPressure::High));
    }

    #[test]
    fn active_render_guard_counts_up_and_down() {
        let base = active_renders();
        let guard = enter_render();
        assert!(active_renders() > base, "进入渲染后活跃计数必须递增");
        drop(guard);
        assert!(active_renders() >= base, "渲染结束后活跃计数必须回落");
    }
}
