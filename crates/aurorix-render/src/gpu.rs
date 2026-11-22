use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;

use aurorix_log::{module, Level};
use skia_safe::{ImageInfo, Surface};

pub const GPU_ENV: &str = "AURORIX_GPU";

pub const GPU_FEATURE_ENABLED: bool = cfg!(feature = "gpu");

const MODULE: &str = "render";

#[cfg(feature = "gpu")]
const SELF_CHECK_SIDE: i32 = 16;

static FAILED: AtomicBool = AtomicBool::new(false);

static FAILURE_REASON: OnceLock<String> = OnceLock::new();

static FALLBACKS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuMode {
    #[default]
    Auto,
    On,
    Off,
}

impl GpuMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            GpuMode::Auto => "auto",
            GpuMode::On => "on",
            GpuMode::Off => "off",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" | "default" => Some(GpuMode::Auto),
            "on" | "force" | "true" | "yes" | "1" => Some(GpuMode::On),
            "off" | "disable" | "disabled" | "false" | "no" | "0" => Some(GpuMode::Off),
            _ => None,
        }
    }

    pub fn from_env() -> Self {
        match std::env::var(GPU_ENV) {
            Ok(raw) => Self::parse(&raw).unwrap_or(GpuMode::Auto),
            Err(_) => GpuMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceKind {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    Other,
}

impl GpuDeviceKind {
    pub const fn label(self) -> &'static str {
        match self {
            GpuDeviceKind::Integrated => "集成显卡",
            GpuDeviceKind::Discrete => "独立显卡",
            GpuDeviceKind::Virtual => "虚拟显卡",
            GpuDeviceKind::Cpu => "软件设备",
            GpuDeviceKind::Other => "其它设备",
        }
    }

    #[cfg(feature = "gpu")]
    const fn rank(self) -> u8 {
        match self {
            GpuDeviceKind::Discrete => 4,
            GpuDeviceKind::Integrated => 3,
            GpuDeviceKind::Virtual => 2,
            GpuDeviceKind::Cpu => 1,
            GpuDeviceKind::Other => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    pub api: &'static str,
    pub device_name: String,
    pub kind: GpuDeviceKind,
    pub vendor_id: u32,
    pub device_id: u32,
    pub driver_version: (u32, u32, u32),
    pub api_version: (u32, u32, u32),
}

impl GpuInfo {
    pub fn label(&self) -> String {
        format!("{} ({})", self.api, self.device_name)
    }

    pub fn api_version_text(&self) -> String {
        let (major, minor, patch) = self.api_version;
        format!("{major}.{minor}.{patch}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuSupport {
    Disabled,
    Unavailable(String),
    Available(GpuInfo),
}

impl GpuSupport {
    pub fn is_available(&self) -> bool {
        matches!(self, GpuSupport::Available(_))
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, GpuSupport::Disabled)
    }

    pub fn info(&self) -> Option<&GpuInfo> {
        match self {
            GpuSupport::Available(info) => Some(info),
            _ => None,
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            GpuSupport::Unavailable(reason) => Some(reason),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            GpuSupport::Disabled => String::from("CPU 多线程光栅 (已按配置关闭 GPU)"),
            GpuSupport::Unavailable(reason) => format!("CPU 多线程光栅 ({reason})"),
            GpuSupport::Available(info) => format!("GPU 硬件加速 {}", info.label()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuRuntimeState {
    pub support: GpuSupport,
    pub failed: bool,
    pub failure: Option<String>,
    pub fallbacks: u64,
}

impl GpuRuntimeState {
    pub fn hardware_accelerated(&self) -> bool {
        self.support.is_available() && !self.failed
    }

    pub fn describe(&self) -> String {
        match (&self.failure, self.support.is_available()) {
            (Some(reason), _) => format!("CPU 多线程光栅 (GPU 运行期回退: {reason})"),
            (None, _) => self.support.describe(),
        }
    }
}

pub fn decode_version(raw: u32) -> (u32, u32, u32) {
    ((raw >> 22) & 0x7f, (raw >> 12) & 0x3ff, raw & 0xfff)
}

pub fn probe_with(mode: GpuMode) -> GpuSupport {
    if mode == GpuMode::Off {
        return GpuSupport::Disabled;
    }
    #[cfg(feature = "gpu")]
    {
        match vulkan::available() {
            Ok(info) => GpuSupport::Available(info),
            Err(reason) => GpuSupport::Unavailable(reason),
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        GpuSupport::Unavailable(String::from("构建时未启用 gpu 特性"))
    }
}

pub fn probe() -> GpuSupport {
    probe_with(GpuMode::from_env())
}

pub fn cached() -> &'static GpuSupport {
    static SUPPORT: OnceLock<GpuSupport> = OnceLock::new();
    SUPPORT.get_or_init(|| {
        let mode = GpuMode::from_env();
        let support = probe_with(mode);
        report(&support, mode);
        support
    })
}

fn report(support: &GpuSupport, mode: GpuMode) {
    let logger = module(MODULE);
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(String::from("mode"), mode.as_str().to_string());
    fields.insert(String::from("backend"), support.describe());
    match support {
        GpuSupport::Available(info) => {
            fields.insert(String::from("device"), info.device_name.clone());
            fields.insert(String::from("kind"), info.kind.label().to_string());
            fields.insert(String::from("api_version"), info.api_version_text());
            logger.log_fields(Level::Info, "渲染硬件加速已启用", fields);
        }
        GpuSupport::Disabled => {
            logger.log_fields(Level::Debug, "渲染使用 CPU 多线程光栅", fields);
        }
        GpuSupport::Unavailable(reason) => {
            let level = match mode {
                GpuMode::On => Level::Warn,
                _ => Level::Debug,
            };
            fields.insert(String::from("reason"), reason.clone());
            logger.log_fields(level, "没有可用的 GPU 上下文, 使用 CPU 多线程光栅", fields);
        }
    }
}

pub fn hardware_accelerated() -> bool {
    cached().is_available() && !FAILED.load(Ordering::SeqCst)
}

pub fn is_usable() -> bool {
    hardware_accelerated()
}

pub fn runtime_state() -> GpuRuntimeState {
    GpuRuntimeState {
        support: cached().clone(),
        failed: FAILED.load(Ordering::SeqCst),
        failure: FAILURE_REASON.get().cloned(),
        fallbacks: FALLBACKS.load(Ordering::SeqCst),
    }
}

pub fn note_failure(reason: &str) {
    FALLBACKS.fetch_add(1, Ordering::SeqCst);
    let first = !FAILED.swap(true, Ordering::SeqCst);
    let _ = FAILURE_REASON.set(reason.to_string());
    if first {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(String::from("reason"), reason.to_string());
        module(MODULE).log_fields(
            Level::Warn,
            "GPU 渲染失败, 后续全部回退 CPU 多线程光栅",
            fields,
        );
    }
}

pub fn fallback_count() -> u64 {
    FALLBACKS.load(Ordering::SeqCst)
}

pub fn reset_failure() -> bool {
    let was = FAILED.swap(false, Ordering::SeqCst);
    FALLBACKS.store(0, Ordering::SeqCst);
    was
}

pub fn ensure_ready() -> Result<(), String> {
    if !hardware_accelerated() {
        return Err(String::from("当前没有可用的 GPU 上下文"));
    }
    #[cfg(feature = "gpu")]
    {
        vulkan::check_session()
    }
    #[cfg(not(feature = "gpu"))]
    {
        Err(String::from("构建时未启用 gpu 特性"))
    }
}

pub fn render_into<R>(
    info: &ImageInfo,
    pixels: &mut [u8],
    row_bytes: usize,
    draw: impl FnOnce(&mut Surface) -> R,
) -> Result<R, String> {
    #[cfg(feature = "gpu")]
    {
        vulkan::render_into(info, pixels, row_bytes, draw)
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = (info, pixels, row_bytes);
        drop(draw);
        Err(String::from("构建时未启用 gpu 特性"))
    }
}

#[cfg(feature = "gpu")]
mod vulkan {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::{Arc, Mutex, OnceLock};

    use ash::vk;
    use ash::vk::Handle as _;
    use skia_safe::gpu::ganesh::surface_ganesh;
    use skia_safe::gpu::ganesh::vk::direct_contexts;
    use skia_safe::gpu::vk::{GetProcOf, Version};
    use skia_safe::gpu::{Budgeted, DirectContext, SurfaceOrigin};
    use skia_safe::{AlphaType, ColorType, ImageInfo, Surface};

    use super::{decode_version, GpuDeviceKind, GpuInfo, SELF_CHECK_SIDE};

    pub const API: &str = "vulkan";

    fn min_api() -> Version {
        Version::new(1, 1, 0)
    }

    static DEVICE: OnceLock<Result<Arc<VulkanDevice>, String>> = OnceLock::new();

    static SUBMIT_LOCK: Mutex<()> = Mutex::new(());

    thread_local! {
        static SESSION: RefCell<Option<GpuSession>> = const { RefCell::new(None) };
    }

    struct GpuSession {
        direct: DirectContext,
    }

    impl GpuSession {
        fn create(device: &VulkanDevice) -> Result<Self, String> {
            let direct = device
                .make_context()
                .ok_or_else(|| String::from("Skia 无法创建 Vulkan 上下文"))?;
            Ok(Self { direct })
        }
    }

    pub(super) struct VulkanDevice {
        entry: ash::Entry,
        instance: ash::Instance,
        device: ash::Device,
        physical: vk::PhysicalDevice,
        queue: vk::Queue,
        queue_family: u32,
        info: GpuInfo,
    }

    unsafe impl Send for VulkanDevice {}

    unsafe impl Sync for VulkanDevice {}

    impl Drop for VulkanDevice {
        fn drop(&mut self) {
            unsafe {
                self.device.destroy_device(None);
                self.instance.destroy_instance(None);
            }
        }
    }

    impl VulkanDevice {
        pub(super) fn shared() -> Result<Arc<VulkanDevice>, String> {
            match DEVICE.get_or_init(Self::create) {
                Ok(device) => Ok(Arc::clone(device)),
                Err(reason) => Err(reason.clone()),
            }
        }

        fn create() -> Result<Arc<VulkanDevice>, String> {
            let entry = unsafe { ash::Entry::load() }
                .map_err(|error| format!("未找到 Vulkan 运行时: {error}"))?;
            let loader_version = unsafe { entry.try_enumerate_instance_version() }
                .map_err(|error| format!("Vulkan 加载器版本探测失败: {error}"))?
                .unwrap_or(vk::API_VERSION_1_0);
            if loader_version < vk::API_VERSION_1_1 {
                return Err(String::from("Vulkan 加载器版本低于 1.1"));
            }
            let app = vk::ApplicationInfo::default()
                .application_name(c"aurorix-render")
                .api_version(vk::API_VERSION_1_1);
            let create_info = vk::InstanceCreateInfo::default().application_info(&app);
            let instance = unsafe { entry.create_instance(&create_info, None) }
                .map_err(|error| format!("创建 Vulkan 实例失败: {error}"))?;
            let candidates = match unsafe { instance.enumerate_physical_devices() } {
                Ok(devices) => devices,
                Err(error) => {
                    unsafe { instance.destroy_instance(None) };
                    return Err(format!("枚举 GPU 设备失败: {error}"));
                }
            };
            let Some((physical, queue_family, kind, properties)) =
                pick_device(&instance, &candidates)
            else {
                unsafe { instance.destroy_instance(None) };
                return Err(String::from("没有支持图形队列的 GPU 设备"));
            };
            let priorities = [1.0f32];
            let queue_info = vk::DeviceQueueCreateInfo::default()
                .queue_family_index(queue_family)
                .queue_priorities(&priorities);
            let device_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(std::slice::from_ref(&queue_info));
            let device = match unsafe { instance.create_device(physical, &device_info, None) } {
                Ok(device) => device,
                Err(error) => {
                    unsafe { instance.destroy_instance(None) };
                    return Err(format!("创建 Vulkan 设备失败: {error}"));
                }
            };
            let queue = unsafe { device.get_device_queue(queue_family, 0) };
            let device_name = properties
                .device_name_as_c_str()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|_| String::from("未知 GPU"));
            let info = GpuInfo {
                api: API,
                device_name,
                kind,
                vendor_id: properties.vendor_id,
                device_id: properties.device_id,
                driver_version: decode_version(properties.driver_version),
                api_version: decode_version(properties.api_version),
            };
            Ok(Arc::new(VulkanDevice {
                entry,
                instance,
                device,
                physical,
                queue,
                queue_family,
                info,
            }))
        }

        fn make_context(&self) -> Option<DirectContext> {
            let get_instance_proc_addr = self.entry.static_fn().get_instance_proc_addr;
            let instance_handle = self.instance.handle();
            let get_proc = move |proc: GetProcOf| -> *const c_void {
                let name = match proc {
                    GetProcOf::Instance(_, name) => name,
                    GetProcOf::Device(_, name) => name,
                };
                unsafe { get_instance_proc_addr(instance_handle, name) }
                    .map_or(ptr::null(), |function| function as *const c_void)
            };
            let backend = unsafe {
                skia_safe::gpu::vk::BackendContext::new_builder(
                    self.instance.handle().as_raw() as usize as _,
                    self.physical.as_raw() as usize as _,
                    self.device.handle().as_raw() as usize as _,
                    (
                        self.queue.as_raw() as usize as _,
                        self.queue_family as usize,
                    ),
                    &get_proc,
                    Some(min_api()),
                )
                .build()
            };
            direct_contexts::make_vulkan(&backend, None)
        }
    }

    fn pick_device(
        instance: &ash::Instance,
        candidates: &[vk::PhysicalDevice],
    ) -> Option<(
        vk::PhysicalDevice,
        u32,
        GpuDeviceKind,
        vk::PhysicalDeviceProperties,
    )> {
        let mut best: Option<(
            u8,
            vk::PhysicalDevice,
            u32,
            GpuDeviceKind,
            vk::PhysicalDeviceProperties,
        )> = None;
        for physical in candidates {
            let properties = unsafe { instance.get_physical_device_properties(*physical) };
            let families =
                unsafe { instance.get_physical_device_queue_family_properties(*physical) };
            let Some(family) = families.iter().enumerate().find_map(|(index, family)| {
                if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                    Some(index as u32)
                } else {
                    None
                }
            }) else {
                continue;
            };
            let kind = device_kind(properties.device_type);
            let rank = kind.rank();
            let better = match &best {
                Some((current, ..)) => rank > *current,
                None => true,
            };
            if better {
                best = Some((rank, *physical, family, kind, properties));
            }
        }
        best.map(|(_, physical, family, kind, properties)| (physical, family, kind, properties))
    }

    const fn device_kind(kind: vk::PhysicalDeviceType) -> GpuDeviceKind {
        match kind {
            vk::PhysicalDeviceType::DISCRETE_GPU => GpuDeviceKind::Discrete,
            vk::PhysicalDeviceType::INTEGRATED_GPU => GpuDeviceKind::Integrated,
            vk::PhysicalDeviceType::VIRTUAL_GPU => GpuDeviceKind::Virtual,
            vk::PhysicalDeviceType::CPU => GpuDeviceKind::Cpu,
            _ => GpuDeviceKind::Other,
        }
    }

    pub(super) fn available() -> Result<GpuInfo, String> {
        let device = VulkanDevice::shared()?;
        Ok(device.info.clone())
    }

    fn with_session<R>(
        body: impl FnOnce(&mut GpuSession) -> Result<R, String>,
    ) -> Result<R, String> {
        SESSION.with(|cell| {
            let mut slot = cell
                .try_borrow_mut()
                .map_err(|_| String::from("GPU 渲染会话被重入调用"))?;
            if slot.is_none() {
                let device = VulkanDevice::shared()?;
                let mut session = GpuSession::create(device.as_ref())?;
                self_check(&mut session)?;
                *slot = Some(session);
            }
            let session = slot
                .as_mut()
                .ok_or_else(|| String::from("GPU 渲染会话初始化失败"))?;
            body(session)
        })
    }

    pub(super) fn check_session() -> Result<(), String> {
        with_session(|_| Ok(()))
    }

    fn submit_and_read(
        direct: &mut DirectContext,
        surface: &mut Surface,
        info: &ImageInfo,
        pixels: &mut [u8],
        row_bytes: usize,
    ) -> Result<(), String> {
        let _guard = SUBMIT_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        direct
            .flush_and_submit()
            .map_err(|error| format!("GPU 提交失败: {error:?}"))?;
        if !surface.read_pixels(info, pixels, row_bytes, (0, 0)) {
            return Err(String::from("GPU 表面回读失败"));
        }
        Ok(())
    }

    fn self_check(session: &mut GpuSession) -> Result<(), String> {
        let info = ImageInfo::new(
            (SELF_CHECK_SIDE, SELF_CHECK_SIDE),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let mut surface = surface_ganesh::render_target(
            &mut session.direct,
            Budgeted::Yes,
            &info,
            0,
            SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        )
        .ok_or_else(|| String::from("GPU 无法分配渲染表面"))?;
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        let row_bytes = info.min_row_bytes();
        let mut pixels = vec![0u8; info.compute_byte_size(row_bytes)];
        submit_and_read(
            &mut session.direct,
            &mut surface,
            &info,
            &mut pixels,
            row_bytes,
        )?;
        Ok(())
    }

    pub(super) fn render_into<R>(
        info: &ImageInfo,
        pixels: &mut [u8],
        row_bytes: usize,
        draw: impl FnOnce(&mut Surface) -> R,
    ) -> Result<R, String> {
        with_session(|session| {
            let mut surface = surface_ganesh::render_target(
                &mut session.direct,
                Budgeted::Yes,
                info,
                0,
                SurfaceOrigin::TopLeft,
                None,
                false,
                false,
            )
            .ok_or_else(|| format!("GPU 无法分配 {}x{} 渲染表面", info.width(), info.height()))?;
            let output = draw(&mut surface);
            submit_and_read(&mut session.direct, &mut surface, info, pixels, row_bytes)?;
            Ok(output)
        })
    }
}
