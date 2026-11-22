use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aurorix_render::fonts;
use aurorix_render::{AssetLoader, RenderError};
use aurorix_template::{Overrides, Registry, ReloadHandle, TemplateKind};
use bytes::Bytes;

pub const DEFAULT_MAX_ASSET_BYTES: u64 = 32 * 1024 * 1024;

pub const FONTS_DIR: &str = "fonts";

pub const TEMPLATES_DIR: &str = "templates";

pub const FONT_SUFFIXES: [&str; 3] = ["ttf", "otf", "ttc"];

pub const TEMPLATE_SUFFIX: &str = "md";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetPathError {
    #[error("素材路径越界: {0}")]
    Escape(String),
    #[error("素材路径不允许是绝对路径: {0}")]
    Absolute(String),
    #[error("素材不存在: {0}")]
    Missing(String),
    #[error("素材读写失败: {0}")]
    Io(String),
    #[error("素材 {key} 有 {size} 字节, 超过上限 {limit} 字节")]
    TooLarge { key: String, size: u64, limit: u64 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginAssetsReport {
    pub fonts: Vec<String>,
    pub templates: Vec<String>,
    pub skipped: Vec<(String, String)>,
    pub root_ready: bool,
}

impl PluginAssetsReport {
    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
            && self.templates.is_empty()
            && self.skipped.is_empty()
            && !self.root_ready
    }

    pub fn registered(&self) -> usize {
        self.fonts.len() + self.templates.len()
    }
}

#[derive(Debug)]
pub struct PluginAssets {
    plugin_id: String,
    root: PathBuf,
    max_bytes: u64,
    registry: Mutex<Option<Arc<Registry>>>,
}

impl PluginAssets {
    pub fn new(plugin_id: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            root: root.into(),
            max_bytes: DEFAULT_MAX_ASSET_BYTES,
            registry: Mutex::new(None),
        }
    }

    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes.max(1);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub fn root_ready(&self) -> bool {
        self.root.is_dir()
    }

    pub fn resolve(&self, key: &str) -> Result<PathBuf, AssetPathError> {
        let relative = relative_parts(key)?;
        let root = self
            .root
            .canonicalize()
            .map_err(|_| AssetPathError::Missing(self.root.display().to_string()))?;
        let mut joined = root.clone();
        for part in &relative {
            joined.push(part);
        }
        let candidate = joined.canonicalize().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => AssetPathError::Missing(key.to_string()),
            _ => AssetPathError::Io(format!("{key}: {error}")),
        })?;
        if !candidate.starts_with(&root) {
            return Err(AssetPathError::Escape(format!(
                "素材 {key} 解析到根目录之外"
            )));
        }
        if !candidate.is_file() {
            return Err(AssetPathError::Missing(key.to_string()));
        }
        Ok(candidate)
    }

    pub fn read(&self, key: &str) -> Result<Vec<u8>, AssetPathError> {
        let path = self.resolve(key)?;
        let size = std::fs::metadata(&path)
            .map_err(|error| AssetPathError::Io(format!("{key}: {error}")))?
            .len();
        if size > self.max_bytes {
            return Err(AssetPathError::TooLarge {
                key: key.to_string(),
                size,
                limit: self.max_bytes,
            });
        }
        let data =
            std::fs::read(&path).map_err(|error| AssetPathError::Io(format!("{key}: {error}")))?;
        if data.len() as u64 > self.max_bytes {
            return Err(AssetPathError::TooLarge {
                key: key.to_string(),
                size: data.len() as u64,
                limit: self.max_bytes,
            });
        }
        Ok(data)
    }

    pub fn exists(&self, key: &str) -> bool {
        self.resolve(key).is_ok()
    }

    pub fn font_alias(&self, name: &str) -> Option<String> {
        let wanted = fonts::normalize_family(&stem_of(name));
        if wanted.is_empty() {
            return None;
        }
        self.fonts()
            .into_iter()
            .find(|alias| fonts::normalize_family(alias) == wanted)
    }

    pub fn fonts(&self) -> Vec<String> {
        self.stems(FONTS_DIR, &FONT_SUFFIXES)
    }

    pub fn templates(&self) -> Vec<String> {
        self.stems(TEMPLATES_DIR, &[TEMPLATE_SUFFIX])
    }

    pub fn loader(self: &Arc<Self>) -> Arc<dyn AssetLoader> {
        Arc::new(PluginAssetLoader {
            assets: Arc::clone(self),
        })
    }

    pub fn bootstrap(&self, templates: &Arc<Registry>) -> PluginAssetsReport {
        let mut report = PluginAssetsReport {
            root_ready: self.root.is_dir(),
            ..PluginAssetsReport::default()
        };
        self.store_registry(Arc::clone(templates));
        for alias in self.fonts() {
            let Some((path, entry)) = self.find_entry(FONTS_DIR, &alias, &FONT_SUFFIXES) else {
                report.skipped.push((
                    format!("{FONTS_DIR}/{alias}"),
                    String::from("字体文件已消失"),
                ));
                continue;
            };
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let faces = fonts::collection_face_count(&bytes);
                    match fonts::register_font_family(&alias, bytes, None) {
                        Ok(()) => {
                            report.fonts.push(alias.clone());
                            for index in 1..faces {
                                report.fonts.push(fonts::collection_alias(&alias, index));
                            }
                        }
                        Err(error) => report.skipped.push((entry, error.to_string())),
                    }
                }
                Err(error) => report.skipped.push((entry, error.to_string())),
            }
        }
        for name in self.templates() {
            let Some((path, entry)) = self.find_entry(TEMPLATES_DIR, &name, &[TEMPLATE_SUFFIX])
            else {
                report.skipped.push((
                    format!("{TEMPLATES_DIR}/{name}"),
                    String::from("模板文件已消失"),
                ));
                continue;
            };
            match std::fs::read(&path) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(content) => {
                        templates.register(
                            TemplateKind::Markdown,
                            &self.plugin_id,
                            &name,
                            &content,
                        );
                        report.templates.push(name);
                    }
                    Err(error) => report
                        .skipped
                        .push((entry, format!("模板不是合法 UTF-8: {error}"))),
                },
                Err(error) => report.skipped.push((entry, error.to_string())),
            }
        }
        report
    }

    pub fn spawn_template_reload(
        self: &Arc<Self>,
        overrides_root: impl Into<PathBuf>,
    ) -> Option<ReloadHandle> {
        let registry = self.registry()?;
        let root = overrides_root.into();
        if root.as_os_str().is_empty() {
            return None;
        }
        let overrides = Arc::new(Overrides::new(
            registry,
            root.to_string_lossy().to_string(),
            Duration::from_secs(aurorix_template::DEFAULT_RELOAD_SECONDS),
        ));
        let _ = overrides.load();
        Some(overrides.spawn_reloader())
    }

    pub fn registry(&self) -> Option<Arc<Registry>> {
        self.registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn store_registry(&self, registry: Arc<Registry>) {
        *self
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(registry);
    }

    fn stems(&self, dir: &str, suffixes: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries(dir, suffixes)
            .into_iter()
            .filter_map(|(_, path)| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn entries(&self, dir: &str, suffixes: &[&str]) -> Vec<(String, PathBuf)> {
        let directory = self.root.join(dir);
        let Ok(read) = std::fs::read_dir(&directory) else {
            return Vec::new();
        };
        let mut found: Vec<(String, PathBuf)> = read
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter(|entry| has_suffix(&entry.path(), suffixes))
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                (name, entry.path())
            })
            .collect();
        found.sort();
        found
    }

    fn find_entry(&self, dir: &str, stem: &str, suffixes: &[&str]) -> Option<(PathBuf, String)> {
        self.entries(dir, suffixes)
            .into_iter()
            .find(|(_, path)| {
                path.file_stem()
                    .map(|value| value.to_string_lossy().eq_ignore_ascii_case(stem))
                    .unwrap_or(false)
            })
            .map(|(name, path)| (path, format!("{dir}/{name}")))
    }
}

impl PartialEq for PluginAssets {
    fn eq(&self, other: &Self) -> bool {
        self.plugin_id == other.plugin_id
            && self.root == other.root
            && self.max_bytes == other.max_bytes
    }
}

impl Eq for PluginAssets {}

struct PluginAssetLoader {
    assets: Arc<PluginAssets>,
}

impl AssetLoader for PluginAssetLoader {
    fn load(&self, key: &str) -> Result<Bytes, RenderError> {
        self.assets.read(key).map(Bytes::from).map_err(|error| {
            RenderError::Texture(format!(
                "插件 {} 的素材 {key} 读取失败: {error}",
                self.assets.plugin_id
            ))
        })
    }
}

fn has_suffix(path: &Path, suffixes: &[&str]) -> bool {
    let Some(extension) = path.extension() else {
        return false;
    };
    let extension = extension.to_string_lossy().to_ascii_lowercase();
    suffixes
        .iter()
        .any(|suffix| extension == suffix.to_ascii_lowercase())
}

fn stem_of(value: &str) -> String {
    Path::new(value)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| value.to_string())
}

fn relative_parts(key: &str) -> Result<Vec<String>, AssetPathError> {
    if key.trim().is_empty() {
        return Err(AssetPathError::Escape(String::from("素材 key 不能为空")));
    }
    if key.contains('\\') {
        return Err(AssetPathError::Escape(format!(
            "素材 key 不允许包含反斜杠: {key}"
        )));
    }
    if key.contains('\0') {
        return Err(AssetPathError::Escape(format!(
            "素材 key 不允许包含 NUL: {key}"
        )));
    }
    if key.starts_with('/') || Path::new(key).is_absolute() || key.as_bytes().get(1) == Some(&b':')
    {
        return Err(AssetPathError::Absolute(key.to_string()));
    }
    let mut parts = Vec::new();
    for component in Path::new(key).components() {
        match component {
            Component::Normal(value) => {
                let part = value.to_str().ok_or_else(|| {
                    AssetPathError::Escape(format!("素材 key 不是合法 UTF-8: {key}"))
                })?;
                if part.is_empty() {
                    continue;
                }
                parts.push(part.to_string());
            }
            Component::CurDir => continue,
            _ => {
                return Err(AssetPathError::Escape(format!(
                    "素材 key 不允许上跳或锚定到根: {key}"
                )))
            }
        }
    }
    if parts.is_empty() {
        return Err(AssetPathError::Escape(format!(
            "素材 key 没有有效分量: {key}"
        )));
    }
    Ok(parts)
}
