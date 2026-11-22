use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const NANO_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

pub const NANO_AVAILABLE_BYTES: u64 = 512 * 1024 * 1024;

pub const COMPACT_LIMIT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub const BALANCED_LIMIT_BYTES: u64 = 8 * 1024 * 1024 * 1024;

pub const FALLBACK_MEMORY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub const PRESSURE_AVAILABLE_BYTES: u64 = 256 * 1024 * 1024;

pub const CGROUP_UNLIMITED_FLOOR: u64 = u64::MAX / 4;

pub const MEMORY_SAMPLE_TTL: Duration = Duration::from_millis(500);
pub const MEMINFO_PATH: &str = "/proc/meminfo";

pub const CGROUP_V2_MAX: &str = "/sys/fs/cgroup/memory.max";

pub const CGROUP_V2_CURRENT: &str = "/sys/fs/cgroup/memory.current";

pub const CGROUP_V1_MAX: &str = "/sys/fs/cgroup/memory/memory.limit_in_bytes";

pub const CGROUP_V1_CURRENT: &str = "/sys/fs/cgroup/memory/memory.usage_in_bytes";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HardwareTier {
    Nano,
    Compact,
    Balanced,
    Performance,
}

impl HardwareTier {
    pub fn label(self) -> &'static str {
        match self {
            HardwareTier::Nano => "微缩",
            HardwareTier::Compact => "紧凑",
            HardwareTier::Balanced => "均衡",
            HardwareTier::Performance => "性能",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            HardwareTier::Nano => "nano",
            HardwareTier::Compact => "compact",
            HardwareTier::Balanced => "balanced",
            HardwareTier::Performance => "performance",
        }
    }

    pub fn of(memory_bytes: u64) -> Self {
        Self::of_pressure(memory_bytes, u64::MAX)
    }

    pub fn of_pressure(memory_bytes: u64, available_bytes: u64) -> Self {
        if memory_bytes <= NANO_LIMIT_BYTES || available_bytes < NANO_AVAILABLE_BYTES {
            HardwareTier::Nano
        } else if memory_bytes <= COMPACT_LIMIT_BYTES {
            HardwareTier::Compact
        } else if memory_bytes <= BALANCED_LIMIT_BYTES {
            HardwareTier::Balanced
        } else {
            HardwareTier::Performance
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CgroupVersion {
    #[default]
    None,
    V1,
    V2,
}

impl CgroupVersion {
    pub fn label(self) -> &'static str {
        match self {
            CgroupVersion::None => "无",
            CgroupVersion::V1 => "v1",
            CgroupVersion::V2 => "v2",
        }
    }

    pub fn detected(self) -> bool {
        self != CgroupVersion::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub cgroup_limit_bytes: Option<u64>,
    pub cgroup_usage_bytes: Option<u64>,
    pub cgroup_version: CgroupVersion,
}

impl Default for MemorySnapshot {
    fn default() -> Self {
        Self {
            total_bytes: FALLBACK_MEMORY_BYTES,
            available_bytes: FALLBACK_MEMORY_BYTES,
            cgroup_limit_bytes: None,
            cgroup_usage_bytes: None,
            cgroup_version: CgroupVersion::None,
        }
    }
}

impl MemorySnapshot {
    pub fn detect() -> Self {
        let (total_bytes, available_bytes) = meminfo();
        let cgroup = cgroup_memory();
        Self {
            total_bytes: match total_bytes {
                0 => FALLBACK_MEMORY_BYTES,
                value => value,
            },
            available_bytes: match available_bytes {
                0 => total_bytes.max(FALLBACK_MEMORY_BYTES),
                value => value,
            },
            cgroup_limit_bytes: cgroup.as_ref().and_then(|source| source.limit_bytes),
            cgroup_usage_bytes: cgroup.as_ref().and_then(|source| source.usage_bytes),
            cgroup_version: cgroup.map(|source| source.version).unwrap_or_default(),
        }
    }

    pub fn from_parts(
        total_bytes: u64,
        available_bytes: u64,
        cgroup_limit_bytes: Option<u64>,
        cgroup_usage_bytes: Option<u64>,
    ) -> Self {
        Self {
            total_bytes,
            available_bytes,
            cgroup_limit_bytes,
            cgroup_usage_bytes,
            cgroup_version: match cgroup_limit_bytes {
                Some(_) => CgroupVersion::V2,
                None => CgroupVersion::None,
            },
        }
    }

    pub fn containerized(&self) -> bool {
        self.cgroup_version.detected() && self.cgroup_limit_bytes.is_some()
    }

    pub fn effective_total_bytes(&self) -> u64 {
        match self.cgroup_limit_bytes {
            Some(limit) => self.total_bytes.min(limit).max(4 * 1024 * 1024),
            None => self.total_bytes.max(4 * 1024 * 1024),
        }
    }

    pub fn effective_available_bytes(&self) -> u64 {
        let system = self.available_bytes;
        let container = match (self.cgroup_limit_bytes, self.cgroup_usage_bytes) {
            (Some(limit), Some(usage)) => limit.saturating_sub(usage),
            (Some(limit), None) => limit,
            _ => u64::MAX,
        };
        system.min(container)
    }

    pub fn total_mib(&self) -> u64 {
        self.total_bytes / (1024 * 1024)
    }

    pub fn available_mib(&self) -> u64 {
        self.available_bytes / (1024 * 1024)
    }

    pub fn effective_total_mib(&self) -> u64 {
        self.effective_total_bytes() / (1024 * 1024)
    }

    pub fn effective_available_mib(&self) -> u64 {
        self.effective_available_bytes() / (1024 * 1024)
    }

    pub fn summary(&self) -> String {
        let container = match self.cgroup_version {
            CgroupVersion::None => "无".to_string(),
            version => format!(
                "{} 上限={}MiB",
                version.label(),
                self.cgroup_limit_bytes.unwrap_or(0) / (1024 * 1024)
            ),
        };
        format!(
            "物理={}MiB 可用={}MiB 有效={}MiB 容器={container}",
            self.total_mib(),
            self.available_mib(),
            self.effective_total_mib()
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryPressure {
    #[default]
    Normal,
    High,
}

impl MemoryPressure {
    pub fn is_high(self) -> bool {
        self == MemoryPressure::High
    }

    pub fn label(self) -> &'static str {
        match self {
            MemoryPressure::Normal => "正常",
            MemoryPressure::High => "高压",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgroupMemory {
    pub limit_bytes: Option<u64>,
    pub usage_bytes: Option<u64>,
    pub version: CgroupVersion,
}

#[derive(Debug, Clone, Copy)]
pub struct HardwareProfile {
    tier: HardwareTier,
    cpus: usize,
    memory_bytes: u64,
    available_bytes: u64,
    cgroup_limit_bytes: Option<u64>,
    cgroup_version: CgroupVersion,
}

impl HardwareProfile {
    pub fn detect() -> Self {
        Self::from_snapshot(cpus(), &memory_snapshot())
    }

    pub fn from_parts(cpus: usize, memory_bytes: u64) -> Self {
        Self {
            tier: HardwareTier::of(memory_bytes),
            cpus: cpus.max(1),
            memory_bytes,
            available_bytes: memory_bytes,
            cgroup_limit_bytes: None,
            cgroup_version: CgroupVersion::None,
        }
    }

    pub fn from_snapshot(cpus: usize, snapshot: &MemorySnapshot) -> Self {
        let memory_bytes = snapshot.effective_total_bytes();
        let available_bytes = snapshot.effective_available_bytes();
        Self {
            tier: HardwareTier::of_pressure(memory_bytes, available_bytes),
            cpus: cpus.max(1),
            memory_bytes,
            available_bytes,
            cgroup_limit_bytes: snapshot.cgroup_limit_bytes,
            cgroup_version: snapshot.cgroup_version,
        }
    }

    pub fn cached() -> &'static Self {
        static PROFILE: OnceLock<HardwareProfile> = OnceLock::new();
        PROFILE.get_or_init(HardwareProfile::detect)
    }

    pub fn tier(&self) -> HardwareTier {
        self.tier
    }

    pub fn cpus(&self) -> usize {
        self.cpus
    }

    pub fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }

    pub fn memory_mib(&self) -> u64 {
        self.memory_bytes / (1024 * 1024)
    }

    pub fn available_bytes(&self) -> u64 {
        self.available_bytes
    }

    pub fn available_mib(&self) -> u64 {
        self.available_bytes / (1024 * 1024)
    }

    pub fn cgroup_limit_bytes(&self) -> Option<u64> {
        self.cgroup_limit_bytes
    }

    pub fn cgroup_version(&self) -> CgroupVersion {
        self.cgroup_version
    }

    pub fn containerized(&self) -> bool {
        self.cgroup_version.detected() && self.cgroup_limit_bytes.is_some()
    }

    pub fn rss_soft_limit_bytes(&self) -> u64 {
        self.rss_soft_limit_for(self.tier)
    }

    pub fn rss_soft_limit_for(&self, tier: HardwareTier) -> u64 {
        match tier {
            HardwareTier::Nano => 192 << 20,
            HardwareTier::Compact => 320 << 20,
            HardwareTier::Balanced => 640 << 20,
            HardwareTier::Performance => 1024 << 20,
        }
    }

    pub fn db_connections(&self) -> u32 {
        self.db_connections_for(self.tier)
    }

    pub fn db_connections_for(&self, tier: HardwareTier) -> u32 {
        match tier {
            HardwareTier::Nano => 2,
            HardwareTier::Compact => 3,
            HardwareTier::Balanced => 8,
            HardwareTier::Performance => 16,
        }
    }

    pub fn render_workers(&self) -> usize {
        self.render_workers_for(self.tier)
    }

    pub fn render_workers_for(&self, tier: HardwareTier) -> usize {
        let wanted = match tier {
            HardwareTier::Nano => 1,
            HardwareTier::Compact => 1,
            HardwareTier::Balanced => 2,
            HardwareTier::Performance => 4,
        };
        wanted.min(self.cpus.max(1))
    }

    pub fn surface_cache(&self) -> usize {
        self.surface_cache_for(self.tier)
    }

    pub fn surface_cache_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 0,
            HardwareTier::Compact => 2,
            HardwareTier::Balanced => 4,
            HardwareTier::Performance => 8,
        }
    }

    pub fn log_ring_capacity(&self) -> usize {
        self.log_ring_capacity_for(self.tier)
    }

    pub fn log_ring_capacity_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 256,
            HardwareTier::Compact => 512,
            HardwareTier::Balanced => 2048,
            HardwareTier::Performance => 4096,
        }
    }

    pub fn log_ring_bytes(&self) -> usize {
        self.log_ring_bytes_for(self.tier)
    }

    pub fn log_ring_bytes_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 1 << 20,
            HardwareTier::Compact => 2 << 20,
            HardwareTier::Balanced => 8 << 20,
            HardwareTier::Performance => 16 << 20,
        }
    }

    pub fn dedup_capacity(&self) -> usize {
        self.dedup_capacity_for(self.tier)
    }

    pub fn dedup_capacity_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 256,
            HardwareTier::Compact => 1024,
            HardwareTier::Balanced => 8192,
            HardwareTier::Performance => 32768,
        }
    }

    pub fn rate_buckets(&self) -> usize {
        self.rate_buckets_for(self.tier)
    }

    pub fn rate_buckets_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 128,
            HardwareTier::Compact => 512,
            HardwareTier::Balanced => 4096,
            HardwareTier::Performance => 16384,
        }
    }

    pub fn session_capacity(&self) -> usize {
        self.session_capacity_for(self.tier)
    }

    pub fn session_capacity_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 4,
            HardwareTier::Compact => 8,
            HardwareTier::Balanced => 32,
            HardwareTier::Performance => 64,
        }
    }

    pub fn name_cache_capacity(&self) -> usize {
        self.name_cache_capacity_for(self.tier)
    }

    pub fn name_cache_capacity_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 256,
            HardwareTier::Compact => 512,
            HardwareTier::Balanced => 4096,
            HardwareTier::Performance => 16384,
        }
    }

    pub fn skia_font_cache_bytes(&self) -> usize {
        self.skia_font_cache_bytes_for(self.tier)
    }

    pub fn skia_font_cache_bytes_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 2 << 20,
            HardwareTier::Compact => 4 << 20,
            HardwareTier::Balanced => 6 << 20,
            HardwareTier::Performance => 8 << 20,
        }
    }

    pub fn skia_resource_cache_bytes(&self) -> usize {
        self.skia_resource_cache_bytes_for(self.tier)
    }

    pub fn skia_resource_cache_bytes_for(&self, tier: HardwareTier) -> usize {
        match tier {
            HardwareTier::Nano => 4 << 20,
            HardwareTier::Compact => 8 << 20,
            HardwareTier::Balanced => 12 << 20,
            HardwareTier::Performance => 16 << 20,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} 档 CPU={} 内存={}MiB 可用={}MiB 容器={} 连接池={} 渲染线程={} 表面池={} 日志环={}条",
            self.tier.label(),
            self.cpus,
            self.memory_mib(),
            self.available_mib(),
            if self.containerized() {
                self.cgroup_version.label()
            } else {
                "无"
            },
            self.db_connections(),
            self.render_workers(),
            self.surface_cache(),
            self.log_ring_capacity()
        )
    }
}

#[derive(Clone)]
struct MemorySample {
    taken_at: Instant,
    snapshot: MemorySnapshot,
    resident_bytes: u64,
}

static SAMPLE: Mutex<Option<MemorySample>> = Mutex::new(None);

fn sample_from(slot: &Mutex<Option<MemorySample>>, ttl: Duration) -> MemorySample {
    let mut guard = slot.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(cached) = guard.as_ref() {
        if cached.taken_at.elapsed() < ttl {
            return cached.clone();
        }
    }
    let fresh = MemorySample {
        taken_at: Instant::now(),
        snapshot: MemorySnapshot::detect(),
        resident_bytes: read_resident_bytes(),
    };
    *guard = Some(fresh.clone());
    fresh
}

fn sample() -> MemorySample {
    sample_from(&SAMPLE, MEMORY_SAMPLE_TTL)
}

pub fn memory_snapshot() -> MemorySnapshot {
    sample().snapshot
}

pub fn invalidate_memory_cache() {
    let mut guard = SAMPLE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    *guard = None;
}

pub fn check_memory_pressure() -> MemoryPressure {
    let sample = sample();
    pressure_of(
        &sample.snapshot,
        sample.resident_bytes,
        HardwareProfile::cached().rss_soft_limit_bytes(),
    )
}

pub fn pressure_of(
    snapshot: &MemorySnapshot,
    rss_bytes: u64,
    rss_limit_bytes: u64,
) -> MemoryPressure {
    if snapshot.effective_available_bytes() < PRESSURE_AVAILABLE_BYTES {
        return MemoryPressure::High;
    }
    if rss_limit_bytes > 0 && rss_bytes > rss_limit_bytes {
        return MemoryPressure::High;
    }
    MemoryPressure::Normal
}

pub fn memory_bytes() -> u64 {
    meminfo().0
}

pub fn available_bytes() -> u64 {
    meminfo().1
}

fn meminfo() -> (u64, u64) {
    let Ok(raw) = std::fs::read_to_string(MEMINFO_PATH) else {
        return (0, 0);
    };
    (
        parse_meminfo(&raw, "MemTotal:"),
        parse_meminfo(&raw, "MemAvailable:"),
    )
}

pub fn parse_meminfo(raw: &str, key: &str) -> u64 {
    for line in raw.lines() {
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let Some(value) = rest.split_whitespace().next() else {
            return 0;
        };
        let Ok(kib) = value.parse::<u64>() else {
            return 0;
        };
        return kib.saturating_mul(1024);
    }
    0
}

pub fn cgroup_memory() -> Option<CgroupMemory> {
    if let Some(source) = cgroup_v2() {
        return Some(source);
    }
    cgroup_v1()
}

fn cgroup_v2() -> Option<CgroupMemory> {
    let limit = parse_cgroup_limit(&std::fs::read_to_string(CGROUP_V2_MAX).ok()?);
    let usage = std::fs::read_to_string(CGROUP_V2_CURRENT)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok());
    if limit.is_none() && usage.is_none() {
        return None;
    }
    Some(CgroupMemory {
        limit_bytes: limit,
        usage_bytes: usage,
        version: CgroupVersion::V2,
    })
}

fn cgroup_v1() -> Option<CgroupMemory> {
    let limit = parse_cgroup_limit(&std::fs::read_to_string(CGROUP_V1_MAX).ok()?);
    let usage = std::fs::read_to_string(CGROUP_V1_CURRENT)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok());
    if limit.is_none() && usage.is_none() {
        return None;
    }
    Some(CgroupMemory {
        limit_bytes: limit,
        usage_bytes: usage,
        version: CgroupVersion::V1,
    })
}

pub fn parse_cgroup_limit(raw: &str) -> Option<u64> {
    let text = raw.trim();
    if text.is_empty() || text == "max" {
        return None;
    }
    let value = text.parse::<u64>().ok()?;
    if value >= CGROUP_UNLIMITED_FLOOR {
        return None;
    }
    Some(value)
}

pub fn cpus() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
}

pub fn resident_bytes() -> u64 {
    sample().resident_bytes
}

fn read_resident_bytes() -> u64 {
    let Ok(raw) = std::fs::read_to_string("/proc/self/statm") else {
        return 0;
    };
    let pages = raw
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    pages.saturating_mul(page_size() as u64)
}

fn page_size() -> usize {
    static SIZE: OnceLock<usize> = OnceLock::new();
    *SIZE.get_or_init(|| unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize })
}

pub fn trim_memory() -> bool {
    platform_trim()
}

#[cfg(target_os = "linux")]
fn platform_trim() -> bool {
    unsafe { libc::malloc_trim(0) != 0 }
}

#[cfg(not(target_os = "linux"))]
fn platform_trim() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str = "MemTotal:        4000000 kB
MemFree:          100000 kB
MemAvailable:     800000 kB
Buffers:           10000 kB
";

    #[test]
    fn tiers_split_on_expected_thresholds() {
        assert_eq!(
            HardwareTier::of(512 * 1024 * 1024),
            HardwareTier::Nano,
            "半 G 内存必须进微缩档"
        );
        assert_eq!(
            HardwareTier::of(NANO_LIMIT_BYTES),
            HardwareTier::Nano,
            "正好 1GiB 属于微缩档"
        );
        assert_eq!(
            HardwareTier::of(NANO_LIMIT_BYTES + 1),
            HardwareTier::Compact
        );
        assert_eq!(
            HardwareTier::of(COMPACT_LIMIT_BYTES),
            HardwareTier::Compact,
            "正好 2GiB 属于紧凑档"
        );
        assert_eq!(
            HardwareTier::of(COMPACT_LIMIT_BYTES + 1),
            HardwareTier::Balanced
        );
        assert_eq!(
            HardwareTier::of(BALANCED_LIMIT_BYTES + 1),
            HardwareTier::Performance
        );
    }

    #[test]
    fn low_available_memory_forces_nano() {
        let tier = HardwareTier::of_pressure(64 * 1024 * 1024 * 1024, 400 * 1024 * 1024);
        assert_eq!(tier, HardwareTier::Nano, "可用内存过低也要进微缩档");
        let relaxed = HardwareTier::of_pressure(64 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024);
        assert_eq!(relaxed, HardwareTier::Performance);
    }

    #[test]
    fn meminfo_parsing_reads_total_and_available() {
        assert_eq!(parse_meminfo(MEMINFO, "MemTotal:"), 4_000_000 * 1024);
        assert_eq!(parse_meminfo(MEMINFO, "MemAvailable:"), 800_000 * 1024);
        assert_eq!(parse_meminfo(MEMINFO, "MemFree:"), 100_000 * 1024);
        assert_eq!(parse_meminfo(MEMINFO, "Missing:"), 0);
        assert_eq!(parse_meminfo("MemTotal: broken\n", "MemTotal:"), 0);
    }

    #[test]
    fn cgroup_limit_parsing_handles_limits_and_sentinels() {
        assert_eq!(parse_cgroup_limit("536870912\n"), Some(512 * 1024 * 1024));
        assert_eq!(parse_cgroup_limit("max\n"), None, "max 表示不限制");
        assert_eq!(parse_cgroup_limit(""), None);
        assert_eq!(parse_cgroup_limit("not-a-number"), None);
        assert_eq!(
            parse_cgroup_limit("9223372036854771712"),
            None,
            "v1 的无限制哨兵值必须当成不限制"
        );
    }

    #[test]
    fn container_limit_caps_effective_total_and_available() {
        let snapshot = MemorySnapshot::from_parts(
            16 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Some(512 * 1024 * 1024),
            Some(256 * 1024 * 1024),
        );
        assert_eq!(snapshot.effective_total_bytes(), 512 * 1024 * 1024);
        assert_eq!(snapshot.effective_available_bytes(), 256 * 1024 * 1024);
        assert_eq!(snapshot.effective_total_mib(), 512);
        assert_eq!(snapshot.effective_available_mib(), 256);
        assert!(snapshot.containerized(), "有 cgroup 上限就算容器内");
        assert!(snapshot.summary().contains("512MiB"));
    }

    #[test]
    fn host_memory_ignores_absent_cgroup() {
        let snapshot =
            MemorySnapshot::from_parts(8 * 1024 * 1024 * 1024, 6 * 1024 * 1024 * 1024, None, None);
        assert_eq!(snapshot.effective_total_bytes(), 8 * 1024 * 1024 * 1024);
        assert_eq!(snapshot.effective_available_bytes(), 6 * 1024 * 1024 * 1024);
        assert!(!snapshot.containerized());
        assert_eq!(snapshot.cgroup_version, CgroupVersion::None);
    }

    #[test]
    fn nano_profile_keeps_zero_surface_cache() {
        let profile = HardwareProfile::from_parts(8, 512 * 1024 * 1024);
        assert_eq!(profile.tier(), HardwareTier::Nano);
        assert_eq!(profile.render_workers(), 1, "微缩档只允许一条渲染线程");
        assert_eq!(profile.surface_cache(), 0, "微缩档不允许驻留画布");
        assert_eq!(profile.db_connections(), 2);
        assert_eq!(profile.skia_font_cache_bytes(), 2 << 20);
        assert_eq!(profile.skia_resource_cache_bytes(), 4 << 20);
        assert_eq!(profile.log_ring_capacity(), 256);
        assert_eq!(profile.dedup_capacity(), 256);
        assert_eq!(profile.rate_buckets(), 128);
        assert_eq!(profile.session_capacity(), 4);
        assert_eq!(profile.name_cache_capacity(), 256);
        assert_eq!(profile.rss_soft_limit_bytes(), 192 << 20);
    }

    #[test]
    fn small_machine_gets_small_limits() {
        let profile = HardwareProfile::from_parts(1, 1536 * 1024 * 1024);
        assert_eq!(profile.tier(), HardwareTier::Compact);
        assert_eq!(profile.db_connections(), 3);
        assert_eq!(profile.render_workers(), 1, "1 核也要留一个线程");
        assert_eq!(profile.log_ring_capacity(), 512);
        assert_eq!(profile.dedup_capacity(), 1024);
        assert_eq!(profile.rate_buckets(), 512);
        assert_eq!(profile.session_capacity(), 8);
        assert_eq!(profile.name_cache_capacity(), 512);
        assert_eq!(profile.surface_cache(), 2);
    }

    #[test]
    fn big_machine_scales_up_but_stays_capped() {
        let profile = HardwareProfile::from_parts(32, 64 * 1024 * 1024 * 1024);
        assert_eq!(profile.tier(), HardwareTier::Performance);
        assert_eq!(profile.render_workers(), 4);
        let two_cores = HardwareProfile::from_parts(2, 64 * 1024 * 1024 * 1024);
        assert_eq!(two_cores.render_workers(), 1, "核数少时线程数跟着核数走");
        let one_core = HardwareProfile::from_parts(1, 64 * 1024 * 1024 * 1024);
        assert_eq!(one_core.render_workers(), 1);
    }

    #[test]
    fn profile_from_snapshot_uses_container_limits() {
        let snapshot = MemorySnapshot::from_parts(
            32 * 1024 * 1024 * 1024,
            16 * 1024 * 1024 * 1024,
            Some(900 * 1024 * 1024),
            Some(100 * 1024 * 1024),
        );
        let profile = HardwareProfile::from_snapshot(16, &snapshot);
        assert_eq!(
            profile.tier(),
            HardwareTier::Nano,
            "容器上限 900MiB 应当进微缩档"
        );
        assert_eq!(profile.memory_mib(), 900);
        assert_eq!(profile.available_mib(), 800);
        assert!(profile.containerized());
        assert!(profile.summary().contains("v2") || profile.summary().contains("容器="));
    }

    #[test]
    fn pressure_triggers_on_low_available_or_high_rss() {
        let tight =
            MemorySnapshot::from_parts(4 * 1024 * 1024 * 1024, 200 * 1024 * 1024, None, None);
        assert_eq!(
            pressure_of(&tight, 10 * 1024 * 1024, 320 << 20),
            MemoryPressure::High,
            "可用内存低于 256MiB 必须判高压"
        );
        let roomy =
            MemorySnapshot::from_parts(4 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024, None, None);
        assert_eq!(
            pressure_of(&roomy, 500 * 1024 * 1024, 320 << 20),
            MemoryPressure::High,
            "RSS 超过软上限也要判高压"
        );
        assert_eq!(
            pressure_of(&roomy, 100 * 1024 * 1024, 320 << 20),
            MemoryPressure::Normal
        );
        assert!(MemoryPressure::High.is_high());
        assert!(!MemoryPressure::Normal.is_high());
    }

    #[test]
    fn resident_bytes_reads_current_process() {
        let rss = resident_bytes();
        assert!(rss > 1024 * 1024, "自己进程的 RSS 不可能小于 1MiB");
    }

    #[test]
    fn live_snapshot_is_consistent() {
        let snapshot = memory_snapshot();
        assert!(snapshot.total_bytes > 0, "物理内存必须能读到");
        assert!(
            snapshot.effective_total_bytes() <= 64 * 1024 * 1024 * 1024 * 1024,
            "有效上限不该是天文数字"
        );
        assert!(snapshot.effective_available_bytes() <= snapshot.effective_total_bytes() * 2);
        let _ = check_memory_pressure();
        let _ = HardwareProfile::detect();
    }

    #[test]
    fn trim_memory_is_callable() {
        let _ = trim_memory();
    }

    #[test]
    fn memory_sample_reuses_cached_reads_within_ttl() {
        let slot: Mutex<Option<MemorySample>> = Mutex::new(None);
        let first = sample_from(&slot, MEMORY_SAMPLE_TTL);
        let second = sample_from(&slot, MEMORY_SAMPLE_TTL);
        assert_eq!(first.taken_at, second.taken_at, "TTL 内必须复用同一次采样");
        assert_eq!(first.resident_bytes, second.resident_bytes);
        std::thread::sleep(MEMORY_SAMPLE_TTL + Duration::from_millis(20));
        let third = sample_from(&slot, MEMORY_SAMPLE_TTL);
        assert!(third.taken_at > first.taken_at, "超过 TTL 必须重新采样");
    }

    #[test]
    fn invalidate_memory_cache_forces_a_new_read() {
        invalidate_memory_cache();
        let first = memory_snapshot();
        invalidate_memory_cache();
        let second = memory_snapshot();
        assert!(first.total_bytes > 0, "失效后仍要能读到物理内存");
        assert_eq!(first.total_bytes, second.total_bytes);
        assert!(resident_bytes() > 0, "失效后仍要能读到 RSS");
    }
}
