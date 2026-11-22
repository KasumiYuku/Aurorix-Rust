use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;

use crate::error::RenderError;
use crate::handle::SharedFontMgr;

pub const TRUETYPE_SIGNATURE: [u8; 4] = [0x00, 0x01, 0x00, 0x00];

pub const CFF_SIGNATURE: [u8; 4] = *b"OTTO";

pub const COLLECTION_SIGNATURE: [u8; 4] = *b"ttcf";

pub const COLLECTION_FACE_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontContainer {
    TrueType,
    Cff,
    Collection,
}

impl FontContainer {
    pub const fn signature(self) -> [u8; 4] {
        match self {
            Self::TrueType => TRUETYPE_SIGNATURE,
            Self::Cff => CFF_SIGNATURE,
            Self::Collection => COLLECTION_SIGNATURE,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::TrueType => "truetype",
            Self::Cff => "opentype-cff",
            Self::Collection => "ttc",
        }
    }

    pub const fn is_collection(self) -> bool {
        matches!(self, Self::Collection)
    }
}

pub fn font_container(data: &[u8]) -> Option<FontContainer> {
    let head = data.get(..4)?;
    [
        FontContainer::TrueType,
        FontContainer::Cff,
        FontContainer::Collection,
    ]
    .into_iter()
    .find(|candidate| head == candidate.signature())
}

pub fn collection_header(data: &[u8]) -> Option<usize> {
    if font_container(data) != Some(FontContainer::Collection) {
        return None;
    }
    let version = data.get(4..8)?;
    let major = u16::from_be_bytes([version[0], version[1]]);
    let minor = u16::from_be_bytes([version[2], version[3]]);
    if minor != 0 || (major != 1 && major != 2) {
        return None;
    }
    let raw = data.get(8..12)?;
    let count = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    Some(count.clamp(1, COLLECTION_FACE_LIMIT))
}

pub fn collection_alias(family: &str, index: usize) -> String {
    format!("{}#{}", normalize_family(family), index)
}

pub fn collection_face_index(family: &str) -> usize {
    match normalize_family(family).rsplit_once('#') {
        Some((prefix, digits)) if !prefix.is_empty() => digits.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn collection_face_count(data: &[u8]) -> usize {
    collection_header(data).unwrap_or(1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFace {
    pub family: Arc<str>,
    pub regular: Arc<Vec<u8>>,
    pub bold: Option<Arc<Vec<u8>>>,
}

impl FontFace {
    pub fn bytes(&self, bold: bool) -> Arc<Vec<u8>> {
        match (&self.bold, bold) {
            (Some(weight), true) => Arc::clone(weight),
            _ => Arc::clone(&self.regular),
        }
    }

    pub fn has_bold(&self) -> bool {
        self.bold.is_some()
    }

    pub fn container(&self) -> Option<FontContainer> {
        font_container(&self.regular)
    }

    pub fn bold_container(&self) -> Option<FontContainer> {
        self.bold.as_deref().and_then(|data| font_container(data))
    }
}

static FAMILIES: LazyLock<Mutex<BTreeMap<String, FontFace>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

static DATA_MANAGER: LazyLock<SharedFontMgr> = LazyLock::new(SharedFontMgr::system);

pub fn normalize_family(family: &str) -> String {
    family
        .chars()
        .filter(|value| !matches!(value, '-' | '_' | ' ' | '\t' | '\r' | '\n'))
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn register_font_family(
    family: &str,
    regular: Vec<u8>,
    bold: Option<Vec<u8>>,
) -> Result<(), RenderError> {
    validate_family(family)?;
    validate_bytes(family, &regular)?;
    if let Some(bold) = bold.as_ref() {
        validate_bytes(family, bold)?;
    }
    insert_collection(family, Arc::new(regular), bold.map(Arc::new)).map(|_| ())
}

pub fn register_font_collection(family: &str, bytes: &[u8]) -> usize {
    if validate_family(family).is_err() || validate_bytes(family, bytes).is_err() {
        return 0;
    }
    insert_collection(family, Arc::new(bytes.to_vec()), None).unwrap_or(0)
}

fn insert_collection(
    family: &str,
    regular: Arc<Vec<u8>>,
    bold: Option<Arc<Vec<u8>>>,
) -> Result<usize, RenderError> {
    insert_family(family, Arc::clone(&regular), bold)?;
    let extra = collection_face_count(&regular).saturating_sub(1);
    for index in 1..=extra {
        insert_family(&collection_alias(family, index), Arc::clone(&regular), None)?;
    }
    Ok(extra + 1)
}

fn insert_family(
    family: &str,
    regular: Arc<Vec<u8>>,
    bold: Option<Arc<Vec<u8>>>,
) -> Result<(), RenderError> {
    validate_family(family)?;
    FAMILIES.lock().insert(
        normalize_family(family),
        FontFace {
            family: Arc::from(family),
            regular,
            bold,
        },
    );
    Ok(())
}

fn validate_family(family: &str) -> Result<(), RenderError> {
    if normalize_family(family).is_empty() || family.contains('\0') {
        return Err(RenderError::Font(String::from(
            "字体家族名不能为空, 也不能包含 NUL",
        )));
    }
    Ok(())
}

fn validate_bytes(family: &str, data: &[u8]) -> Result<(), RenderError> {
    match font_container(data) {
        None => Err(RenderError::Font(format!(
            "字体 {family} 的 {} 字节不是合法的 TrueType/OpenType/TTC 容器",
            data.len()
        ))),
        Some(container) if container.is_collection() && collection_header(data).is_none() => Err(
            RenderError::Font(format!("字体 {family} 的 TTC 头无效, 无法枚举 face")),
        ),
        Some(_) => Ok(()),
    }
}

pub fn font_family_bytes(family: &str, bold: bool) -> Option<Arc<Vec<u8>>> {
    let key = normalize_family(family);
    let guard = FAMILIES.lock();
    guard.get(&key).map(|face| face.bytes(bold))
}

pub fn font_face(family: &str) -> Option<FontFace> {
    let key = normalize_family(family);
    FAMILIES.lock().get(&key).cloned()
}

pub fn has_font_family(family: &str) -> bool {
    let key = normalize_family(family);
    FAMILIES.lock().contains_key(&key)
}

pub fn font_family_names() -> Vec<String> {
    FAMILIES
        .lock()
        .values()
        .map(|face| face.family.to_string())
        .collect()
}

pub fn font_family_count() -> usize {
    FAMILIES.lock().len()
}

pub fn clear_font_families() {
    FAMILIES.lock().clear();
}

pub fn data_font_manager() -> &'static SharedFontMgr {
    &DATA_MANAGER
}
