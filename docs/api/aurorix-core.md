# aurorix-core API 索引

> 本文件由 `cargo run -p xtask -- gen-api` 从源码生成，请勿手改。完整签名与泛型约束用 `cargo doc --workspace --no-deps --open` 查看。

## `src/constant.rs`

- `pub const DEFAULT_MAINTENANCE_MESSAGE: &str = "当前该功能模块正在维护中，请等待维护完成再重试"`
- `pub const DEFAULT_DENIED_MESSAGE: &str = "你没有权限使用该指令"`
- `pub const DEFAULT_UPLOAD_THRESHOLD: usize = 3 << 20`
- `pub const DEFAULT_IMAGE_MAX_WIDTH: u32 = 350`
- `pub const DEFAULT_IMAGE_MAX_HEIGHT: u32 = 438`
- `pub enum MessageType`
- `impl MessageType`
  - `pub const fn code(self) -> u8`
- `pub mod media_kind`
- `pub const IMAGE: u8 = 1`
- `pub const VIDEO: u8 = 2`
- `pub const VOICE: u8 = 3`
- `pub const FILE: u8 = 4`

## `src/error.rs`

- `pub enum CoreError`
- `pub type CoreResult<T> = Result<T, CoreError>`

## `src/lib.rs`

- `pub use constant::{ DEFAULT_DENIED_MESSAGE, DEFAULT_IMAGE_MAX_HEIGHT, DEFAULT_IMAGE_MAX_WIDTH, DEFAULT_MAINTENANCE_MESSAGE, DEFAULT_UPLOAD_THRESHOLD, }`
- `pub use error::{CoreError, CoreResult}`
- `pub use prefix::PrefixBook`
- `pub use role::RoleRequired`
- `pub use scene::{Scene, Target}`
- `pub use sys::{ available_bytes, cgroup_memory, check_memory_pressure, cpus, memory_bytes, memory_snapshot, parse_cgroup_limit, parse_meminfo, pressure_of, resident_bytes, trim_memory, CgroupMemory, CgroupVersion, HardwareProfile, HardwareTier, MemoryPressure, MemorySnapshot, PRESSURE_AVAILABLE_BYTES, }`

## `src/prefix.rs`

- `pub fn default_prefix_chars() -> Vec<String>`
- `pub struct PrefixBook`
- `impl PrefixBook`
  - `pub fn new() -> Self`
  - `pub fn global() -> &'static PrefixBook`
  - `pub fn get(&self) -> Arc<Vec<String>>`
  - `pub fn set(&self, chars: Vec<String>)`
  - `pub fn has_bare(&self) -> bool`

## `src/role.rs`

- `pub enum RoleRequired`
- `impl RoleRequired`
  - `pub const fn as_str(self) -> &'static str`
  - `pub const fn can_use(self, user: Option<RoleRequired>) -> bool`

## `src/scene.rs`

- `pub enum Scene`
- `impl Scene`
  - `pub const ALL: [Scene; 4] = [Scene::Group, Scene::C2c, Scene::Channel, Scene::Dm]`
  - `pub const fn as_str(self) -> &'static str`
  - `pub const fn label(self) -> &'static str`
  - `pub const fn guild(self) -> bool`
  - `pub fn valid(self) -> bool`
- `pub struct Target`
- `impl Target`
  - `pub fn new(scene: Scene, id: impl Into<String>) -> Self`
  - `pub fn with_user(mut self, user: impl Into<String>) -> Self`
  - `pub fn with_mention_id(mut self, mention_id: impl Into<String>) -> Self`
  - `pub fn mention(&self) -> &str`
  - `pub fn empty(&self) -> bool`

## `src/sys.rs`

- `pub const NANO_LIMIT_BYTES: u64 = 1024 * 1024 * 1024`
- `pub const NANO_AVAILABLE_BYTES: u64 = 512 * 1024 * 1024`
- `pub const COMPACT_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024`
- `pub const BALANCED_LIMIT_BYTES: u64 = 8 * 1024 * 1024 * 1024`
- `pub const FALLBACK_MEMORY_BYTES: u64 = 2 * 1024 * 1024 * 1024`
- `pub const PRESSURE_AVAILABLE_BYTES: u64 = 256 * 1024 * 1024`
- `pub const CGROUP_UNLIMITED_FLOOR: u64 = u64::MAX / 4`
- `pub const MEMINFO_PATH: &str = "/proc/meminfo"`
- `pub const CGROUP_V2_MAX: &str = "/sys/fs/cgroup/memory.max"`
- `pub const CGROUP_V2_CURRENT: &str = "/sys/fs/cgroup/memory.current"`
- `pub const CGROUP_V1_MAX: &str = "/sys/fs/cgroup/memory/memory.limit_in_bytes"`
- `pub const CGROUP_V1_CURRENT: &str = "/sys/fs/cgroup/memory/memory.usage_in_bytes"`
- `pub enum HardwareTier`
- `impl HardwareTier`
  - `pub fn label(self) -> &'static str`
  - `pub fn as_str(self) -> &'static str`
  - `pub fn of(memory_bytes: u64) -> Self`
  - `pub fn of_pressure(memory_bytes: u64, available_bytes: u64) -> Self`
- `pub enum CgroupVersion`
- `impl CgroupVersion`
  - `pub fn label(self) -> &'static str`
  - `pub fn detected(self) -> bool`
- `pub struct MemorySnapshot`
- `impl MemorySnapshot`
  - `pub fn detect() -> Self`
  - `pub fn from_parts( …`
  - `pub fn containerized(&self) -> bool`
  - `pub fn effective_total_bytes(&self) -> u64`
  - `pub fn effective_available_bytes(&self) -> u64`
  - `pub fn total_mib(&self) -> u64`
  - `pub fn available_mib(&self) -> u64`
  - `pub fn effective_total_mib(&self) -> u64`
  - `pub fn effective_available_mib(&self) -> u64`
  - `pub fn summary(&self) -> String`
- `pub enum MemoryPressure`
- `impl MemoryPressure`
  - `pub fn is_high(self) -> bool`
  - `pub fn label(self) -> &'static str`
- `pub struct CgroupMemory`
- `pub struct HardwareProfile`
- `impl HardwareProfile`
  - `pub fn detect() -> Self`
  - `pub fn from_parts(cpus: usize, memory_bytes: u64) -> Self`
  - `pub fn from_snapshot(cpus: usize, snapshot: &MemorySnapshot) -> Self`
  - `pub fn cached() -> &'static Self`
  - `pub fn tier(&self) -> HardwareTier`
  - `pub fn cpus(&self) -> usize`
  - `pub fn memory_bytes(&self) -> u64`
  - `pub fn memory_mib(&self) -> u64`
  - `pub fn available_bytes(&self) -> u64`
  - `pub fn available_mib(&self) -> u64`
  - `pub fn cgroup_limit_bytes(&self) -> Option<u64>`
  - `pub fn cgroup_version(&self) -> CgroupVersion`
  - `pub fn containerized(&self) -> bool`
  - `pub fn rss_soft_limit_bytes(&self) -> u64`
  - `pub fn rss_soft_limit_for(&self, tier: HardwareTier) -> u64`
  - `pub fn db_connections(&self) -> u32`
  - `pub fn db_connections_for(&self, tier: HardwareTier) -> u32`
  - `pub fn render_workers(&self) -> usize`
  - `pub fn render_workers_for(&self, tier: HardwareTier) -> usize`
  - `pub fn surface_cache(&self) -> usize`
  - `pub fn surface_cache_for(&self, tier: HardwareTier) -> usize`
  - `pub fn log_ring_capacity(&self) -> usize`
  - `pub fn log_ring_capacity_for(&self, tier: HardwareTier) -> usize`
  - `pub fn log_ring_bytes(&self) -> usize`
  - `pub fn log_ring_bytes_for(&self, tier: HardwareTier) -> usize`
  - `pub fn dedup_capacity(&self) -> usize`
  - `pub fn dedup_capacity_for(&self, tier: HardwareTier) -> usize`
  - `pub fn rate_buckets(&self) -> usize`
  - `pub fn rate_buckets_for(&self, tier: HardwareTier) -> usize`
  - `pub fn session_capacity(&self) -> usize`
  - `pub fn session_capacity_for(&self, tier: HardwareTier) -> usize`
  - `pub fn name_cache_capacity(&self) -> usize`
  - `pub fn name_cache_capacity_for(&self, tier: HardwareTier) -> usize`
  - `pub fn skia_font_cache_bytes(&self) -> usize`
  - `pub fn skia_font_cache_bytes_for(&self, tier: HardwareTier) -> usize`
  - `pub fn skia_resource_cache_bytes(&self) -> usize`
  - `pub fn skia_resource_cache_bytes_for(&self, tier: HardwareTier) -> usize`
  - `pub fn summary(&self) -> String`
- `pub fn memory_snapshot() -> MemorySnapshot`
- `pub fn check_memory_pressure() -> MemoryPressure`
- `pub fn pressure_of( …`
- `pub fn memory_bytes() -> u64`
- `pub fn available_bytes() -> u64`
- `pub fn parse_meminfo(raw: &str, key: &str) -> u64`
- `pub fn cgroup_memory() -> Option<CgroupMemory>`
- `pub fn parse_cgroup_limit(raw: &str) -> Option<u64>`
- `pub fn cpus() -> usize`
- `pub fn resident_bytes() -> u64`
- `pub fn trim_memory() -> bool`

