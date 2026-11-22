use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;

use crate::registry::{BaseKey, Registry, TemplateKind};

pub const GLOBAL_NAMESPACE: &str = "_global";

pub const DEFAULT_OVERRIDE_DIR: &str = "templates/override";

pub const DEFAULT_RELOAD_SECONDS: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileState {
    size: u64,
    modified: Option<SystemTime>,
}

pub struct Overrides {
    registry: Arc<Registry>,
    root: String,
    interval: Duration,
    seen: Mutex<std::collections::BTreeMap<BaseKey, FileState>>,
    last_reload: Mutex<Option<Instant>>,
    reload_count: AtomicU64,
}

impl std::fmt::Debug for Overrides {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overrides")
            .field("root", &self.root)
            .field("interval", &self.interval)
            .finish()
    }
}

pub struct ReloadHandle {
    handle: Option<tokio::task::JoinHandle<()>>,
    owner: Arc<Overrides>,
}

impl ReloadHandle {
    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }

    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .map(|handle| !handle.is_finished())
            .unwrap_or(false)
    }

    pub fn overrides(&self) -> &Arc<Overrides> {
        &self.owner
    }
}

impl Drop for ReloadHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

impl std::fmt::Debug for ReloadHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReloadHandle")
            .field("running", &self.is_running())
            .finish()
    }
}

impl Overrides {
    pub fn new(registry: Arc<Registry>, root: impl Into<String>, interval: Duration) -> Self {
        let interval = if interval.is_zero() {
            Duration::from_secs(DEFAULT_RELOAD_SECONDS)
        } else {
            interval
        };
        Self {
            registry,
            root: root.into().trim().to_string(),
            interval,
            seen: Mutex::new(std::collections::BTreeMap::new()),
            last_reload: Mutex::new(None),
            reload_count: AtomicU64::new(0),
        }
    }

    pub fn with_global(registry: Arc<Registry>, root: impl Into<String>) -> Self {
        Self::new(registry, root, Duration::from_secs(DEFAULT_RELOAD_SECONDS))
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn load(&self) -> io::Result<()> {
        let mut guard = self
            .seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.scan(&mut guard);
        drop(guard);
        self.reload_count.fetch_add(1, Ordering::SeqCst);
        *self
            .last_reload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        Ok(())
    }

    pub fn last_reload(&self) -> Option<Instant> {
        *self
            .last_reload
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn reload_count(&self) -> u64 {
        self.reload_count.load(Ordering::SeqCst)
    }

    pub fn spawn_reloader(self: &Arc<Self>) -> ReloadHandle {
        let interval = self.interval;
        let owner = Arc::downgrade(self);
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(overrides) = owner.upgrade() else {
                    break;
                };
                let _ = overrides.load();
            }
        });
        ReloadHandle {
            handle: Some(handle),
            owner: Arc::clone(self),
        }
    }

    fn scan(&self, seen: &mut std::collections::BTreeMap<BaseKey, FileState>) {
        if self.root.is_empty() {
            return;
        }
        let root = Path::new(&self.root);
        let usable = std::fs::metadata(root)
            .map(|meta| meta.is_dir())
            .unwrap_or(false);
        if !usable {
            self.revert_all(seen);
            return;
        }
        let mut files: Vec<PathBuf> = Vec::new();
        collect_files(root, &mut files);
        let mut next = std::collections::BTreeMap::new();
        for path in files {
            let Some(key) = override_key(root, &path) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let state = FileState {
                size: meta.len(),
                modified: meta.modified().ok(),
            };
            if seen.get(&key) == Some(&state) {
                next.insert(key, state);
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            self.registry.put(key.kind, &key.ns, &key.id, &content);
            next.insert(key, state);
        }
        let stale: Vec<BaseKey> = seen
            .keys()
            .filter(|key| !next.contains_key(*key))
            .cloned()
            .collect();
        for key in stale {
            self.revert(&key);
        }
        *seen = next;
    }

    fn revert_all(&self, seen: &mut std::collections::BTreeMap<BaseKey, FileState>) {
        let keys: Vec<BaseKey> = seen.keys().cloned().collect();
        for key in keys {
            self.revert(&key);
        }
        seen.clear();
    }

    fn revert(&self, key: &BaseKey) {
        match self.registry.base_of(key.kind, &key.ns, &key.id) {
            Some(content) => self.registry.put(key.kind, &key.ns, &key.id, &content),
            None => self.registry.drop_template(key.kind, &key.ns, &key.id),
        }
    }

    pub async fn watch(self: Arc<Self>) {
        if self.root.is_empty() {
            return;
        }
        loop {
            tokio::time::sleep(self.interval).await;
            let _ = self.load();
        }
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

pub(crate) fn override_key(root: &Path, path: &Path) -> Option<BaseKey> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect();
    if parts.len() != 2 {
        return None;
    }
    let ns = if parts[0] == GLOBAL_NAMESPACE {
        String::new()
    } else {
        parts[0].clone()
    };
    let name = &parts[1];
    if let Some(id) = name.strip_suffix(".md") {
        return Some(BaseKey {
            ns,
            kind: TemplateKind::Markdown,
            id: id.to_string(),
        });
    }
    if let Some(id) = name.strip_suffix(".html") {
        return Some(BaseKey {
            ns,
            kind: TemplateKind::Html,
            id: id.to_string(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args_map;

    fn write_file(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }

    #[test]
    fn override_applies_then_reverts_to_base() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Markdown, "", "ovr", "base {{v}}");
        let overrides = Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_secs(1),
        );
        write_file(dir.path(), "_global/ovr.md", "override {{v}}");
        overrides.load().expect("load");
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "ovr", &args_map! { "v" => "X" })
                .unwrap(),
            "override X"
        );
        std::fs::remove_file(dir.path().join("_global/ovr.md")).expect("remove");
        overrides.load().expect("reload");
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "ovr", &args_map! { "v" => "X" })
                .unwrap(),
            "base X"
        );
    }

    #[test]
    fn override_content_change_is_picked_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Markdown, "", "chg", "base {{v}}");
        let overrides = Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_secs(1),
        );
        write_file(dir.path(), "_global/chg.md", "one {{v}}");
        overrides.load().expect("load");
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "chg", &args_map! { "v" => "X" })
                .unwrap(),
            "one X"
        );
        write_file(dir.path(), "_global/chg.md", "two {{v}}");
        overrides.load().expect("reload");
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "chg", &args_map! { "v" => "X" })
                .unwrap(),
            "two X"
        );
    }

    #[test]
    fn override_adds_template_without_base_and_drops_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        let overrides = Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_secs(1),
        );
        write_file(dir.path(), "_global/extra.md", "extra {{v}}");
        overrides.load().expect("load");
        assert!(reg.exists(TemplateKind::Markdown, "extra"));
        std::fs::remove_file(dir.path().join("_global/extra.md")).expect("remove");
        overrides.load().expect("reload");
        assert!(!reg.exists(TemplateKind::Markdown, "extra"));
    }

    #[test]
    fn missing_directory_keeps_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Markdown, "", "keep", "base {{v}}");
        let overrides = Overrides::new(
            reg.clone(),
            dir.path().join("not-exist").to_string_lossy(),
            Duration::from_secs(1),
        );
        overrides.load().expect("load");
        assert_eq!(
            reg.fill(
                TemplateKind::Markdown,
                "",
                "keep",
                &args_map! { "v" => "X" }
            )
            .unwrap(),
            "base X"
        );
    }

    #[test]
    fn empty_root_is_noop() {
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Markdown, "", "empty", "base {{v}}");
        let overrides = Overrides::new(reg.clone(), "", Duration::from_secs(1));
        overrides.load().expect("load");
        assert_eq!(overrides.interval(), Duration::from_secs(1));
        assert_eq!(overrides.root(), "");
        assert_eq!(
            reg.fill(
                TemplateKind::Markdown,
                "",
                "empty",
                &args_map! { "v" => "X" }
            )
            .unwrap(),
            "base X"
        );
    }

    #[test]
    fn namespace_overrides_stay_isolated() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Markdown, "", "iso", "base {{v}}");
        let overrides = Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_secs(1),
        );
        write_file(dir.path(), "arcaea/iso.md", "scoped {{v}}");
        overrides.load().expect("load");
        assert_eq!(
            reg.fill(TemplateKind::Markdown, "", "iso", &args_map! { "v" => "X" })
                .unwrap(),
            "base X"
        );
        assert_eq!(
            reg.fill(
                TemplateKind::Markdown,
                "arcaea",
                "iso",
                &args_map! { "v" => "X" }
            )
            .unwrap(),
            "scoped X"
        );
    }

    #[test]
    fn html_override_uses_html_suffix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Html, "", "page", "base {{v}}");
        let overrides = Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_secs(1),
        );
        write_file(dir.path(), "_global/page.html", "html {{v}}");
        overrides.load().expect("load");
        assert_eq!(
            reg.fill(TemplateKind::Html, "", "page", &args_map! { "v" => "X" })
                .unwrap(),
            "html X"
        );
    }

    #[tokio::test]
    async fn watcher_applies_changes_on_interval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Markdown, "", "watched", "base {{v}}");
        let overrides = Arc::new(Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_millis(20),
        ));
        let handle = tokio::spawn(overrides.watch());
        write_file(dir.path(), "_global/watched.md", "watched {{v}}");
        let mut applied = false;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if reg
                .fill(
                    TemplateKind::Markdown,
                    "",
                    "watched",
                    &args_map! { "v" => "X" },
                )
                .unwrap()
                == "watched X"
            {
                applied = true;
                break;
            }
        }
        handle.abort();
        assert!(applied);
    }

    #[test]
    fn files_not_matching_layout_are_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        let overrides = Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_secs(1),
        );
        write_file(dir.path(), "_global/nested/deep.md", "x");
        write_file(dir.path(), "_global/ignored.txt", "x");
        overrides.load().expect("load");
        assert!(!reg.exists(TemplateKind::Markdown, "deep"));
        assert!(!reg.exists(TemplateKind::Markdown, "ignored"));
    }

    async fn wait_until(mut ready: impl FnMut() -> bool, label: &str) {
        for _ in 0..200 {
            if ready() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("等待超时: {label}");
    }

    #[tokio::test]
    async fn reloader_applies_disk_changes_to_the_next_render() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reg = Arc::new(Registry::new());
        reg.register(TemplateKind::Markdown, "hot", "live", "base {{v}}");
        let overrides = Arc::new(Overrides::new(
            reg.clone(),
            dir.path().to_string_lossy(),
            Duration::from_millis(50),
        ));
        let mut handle = overrides.spawn_reloader();
        assert!(handle.is_running(), "驱动器应当在运行");
        let render = |registry: &Registry| {
            registry
                .fill(
                    TemplateKind::Markdown,
                    "hot",
                    "live",
                    &args_map! { "v" => "X" },
                )
                .expect("模板可用")
        };
        assert_eq!(render(&reg), "base X", "还没落盘时用基础模板");

        write_file(dir.path(), "hot/live.md", "first {{v}}");
        wait_until(|| render(&reg) == "first X", "第一次磁盘覆盖生效").await;

        write_file(dir.path(), "hot/live.md", "second-longer {{v}}");
        wait_until(|| render(&reg) == "second-longer X", "第二次磁盘覆盖生效").await;

        assert!(
            overrides.reload_count() >= 2,
            "每轮扫描都要计数, 实际 {}",
            overrides.reload_count()
        );
        assert!(
            overrides.last_reload().is_some(),
            "应当记录最近一次扫描时刻"
        );

        handle.stop();
        assert!(!handle.is_running(), "stop 之后驱动器应当停止");
    }
}
