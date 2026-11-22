use skia_safe::{Data, FontMgr, FontStyle, Image, Typeface};

use crate::fonts::COLLECTION_FACE_LIMIT;

pub fn font_style(bold: bool) -> FontStyle {
    if bold {
        FontStyle::bold()
    } else {
        FontStyle::normal()
    }
}

pub struct SharedFontMgr(FontMgr);

unsafe impl Send for SharedFontMgr {}
unsafe impl Sync for SharedFontMgr {}

impl SharedFontMgr {
    pub fn system() -> Self {
        Self(FontMgr::default())
    }

    pub fn raw(&self) -> &FontMgr {
        &self.0
    }

    pub fn family_count(&self) -> usize {
        self.0.count_families()
    }

    pub fn typeface_from_data(&self, data: &[u8]) -> Option<SharedTypeface> {
        self.typeface_at(data, 0)
    }

    pub fn typeface_at(&self, data: &[u8], index: usize) -> Option<SharedTypeface> {
        if data.is_empty() {
            return None;
        }
        let face_index = u32::try_from(index).ok()?;
        self.0
            .new_from_data(Data::new_copy(data), face_index)
            .map(SharedTypeface::from_typeface)
    }

    pub fn typeface_from_collection(&self, data: &[u8]) -> Vec<SharedTypeface> {
        let mut faces = Vec::new();
        if data.is_empty() {
            return faces;
        }
        for index in 0..COLLECTION_FACE_LIMIT {
            match self.typeface_at(data, index) {
                Some(face) => faces.push(face),
                None => break,
            }
        }
        faces
    }

    pub fn match_family(&self, family: &str, bold: bool) -> Option<SharedTypeface> {
        if family.is_empty() || family.contains('\0') {
            return None;
        }
        self.0
            .match_family_style(family, font_style(bold))
            .map(SharedTypeface::from_typeface)
    }

    pub fn default_typeface(&self, bold: bool) -> Option<SharedTypeface> {
        self.0
            .legacy_make_typeface(None, font_style(bold))
            .map(SharedTypeface::from_typeface)
    }
}

impl std::fmt::Debug for SharedFontMgr {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedFontMgr")
            .field("families", &self.family_count())
            .finish()
    }
}

#[derive(Clone)]
pub struct SharedTypeface(Typeface);

unsafe impl Send for SharedTypeface {}
unsafe impl Sync for SharedTypeface {}

impl SharedTypeface {
    pub fn from_typeface(typeface: Typeface) -> Self {
        Self(typeface)
    }

    pub fn typeface(&self) -> &Typeface {
        &self.0
    }

    pub fn duplicate(&self) -> Typeface {
        self.0.clone()
    }

    pub fn family_name(&self) -> String {
        self.0.family_name()
    }
}

impl std::fmt::Debug for SharedTypeface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedTypeface")
            .field("family", &self.family_name())
            .finish()
    }
}

pub struct SharedImage(Image);

unsafe impl Send for SharedImage {}
unsafe impl Sync for SharedImage {}

impl SharedImage {
    pub fn from_image(image: Image) -> Self {
        Self(image)
    }

    pub fn image(&self) -> &Image {
        &self.0
    }

    pub fn duplicate(&self) -> Image {
        self.0.clone()
    }

    pub fn dimensions(&self) -> (i32, i32) {
        (self.0.width(), self.0.height())
    }
}

impl std::fmt::Debug for SharedImage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (width, height) = self.dimensions();
        formatter
            .debug_struct("SharedImage")
            .field("width", &width)
            .field("height", &height)
            .finish()
    }
}
