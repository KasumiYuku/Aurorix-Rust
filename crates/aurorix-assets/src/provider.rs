use std::ops::BitOr;

use aurorix_core::Target;

use crate::error::AssetError;
pub use crate::http::BoxFuture;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

impl MediaKind {
    pub const ALL: [MediaKind; 3] = [MediaKind::Image, MediaKind::Video, MediaKind::Audio];

    pub const fn bit(self) -> u8 {
        match self {
            MediaKind::Image => 1,
            MediaKind::Video => 2,
            MediaKind::Audio => 4,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            MediaKind::Image => "图片",
            MediaKind::Video => "视频",
            MediaKind::Audio => "音频",
        }
    }

    pub fn of_mime(mime: &str) -> Self {
        if mime.starts_with("video/") {
            MediaKind::Video
        } else if mime.starts_with("audio/") {
            MediaKind::Audio
        } else {
            MediaKind::Image
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaKinds(u8);

impl MediaKinds {
    pub const NONE: MediaKinds = MediaKinds(0);
    pub const IMAGE: MediaKinds = MediaKinds(1);
    pub const VIDEO: MediaKinds = MediaKinds(2);
    pub const AUDIO: MediaKinds = MediaKinds(4);

    pub const fn all() -> Self {
        MediaKinds(7)
    }

    pub fn from_bits(bits: u8) -> Self {
        MediaKinds(bits & 7)
    }

    pub fn contains(self, kind: MediaKind) -> bool {
        self.0 & kind.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn bits(self) -> u8 {
        self.0
    }
}

impl BitOr for MediaKinds {
    type Output = MediaKinds;

    fn bitor(self, rhs: MediaKinds) -> MediaKinds {
        MediaKinds(self.0 | rhs.0)
    }
}

impl From<MediaKind> for MediaKinds {
    fn from(kind: MediaKind) -> Self {
        MediaKinds(kind.bit())
    }
}

impl Default for MediaKinds {
    fn default() -> Self {
        MediaKinds::IMAGE
    }
}

#[derive(Clone, Debug)]
pub struct UploadInput<'a> {
    pub bytes: &'a [u8],
    pub filename: String,
    pub mime: String,
    pub target: Target,
    pub trace: String,
    pub command: String,
}

impl<'a> UploadInput<'a> {
    pub fn new(bytes: &'a [u8], filename: impl Into<String>, mime: impl Into<String>) -> Self {
        Self {
            bytes,
            filename: filename.into(),
            mime: mime.into(),
            target: Target::default(),
            trace: String::new(),
            command: String::new(),
        }
    }

    pub fn with_target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    pub fn with_trace(mut self, trace: impl Into<String>) -> Self {
        self.trace = trace.into();
        self
    }

    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = command.into();
        self
    }

    pub fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
}

#[derive(Clone, Debug, Default)]
pub struct UploadContext<'a> {
    pub target: Target,
    pub trace: &'a str,
    pub command: &'a str,
}

impl<'a> UploadContext<'a> {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            trace: "",
            command: "",
        }
    }

    pub fn with_trace(mut self, trace: &'a str) -> Self {
        self.trace = trace;
        self
    }

    pub fn with_command(mut self, command: &'a str) -> Self {
        self.command = command;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedImage {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub provider: String,
    pub session_scoped: bool,
}

impl ResolvedImage {
    pub fn passthrough(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    pub fn uploaded(
        url: impl Into<String>,
        provider: impl Into<String>,
        session_scoped: bool,
    ) -> Self {
        Self {
            url: url.into(),
            width: 0,
            height: 0,
            provider: provider.into(),
            session_scoped,
        }
    }

    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn is_passthrough(&self) -> bool {
        self.provider.is_empty()
    }
}

pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn max_size(&self) -> u64 {
        0
    }

    fn session_scoped(&self) -> bool {
        false
    }

    fn skip_probe(&self) -> bool {
        false
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct FixedProvider {
        name: &'static str,
        kinds: MediaKinds,
        limit: u64,
    }

    impl Provider for FixedProvider {
        fn name(&self) -> &str {
            self.name
        }

        fn kinds(&self) -> MediaKinds {
            self.kinds
        }

        fn max_size(&self) -> u64 {
            self.limit
        }

        fn upload<'a>(
            &'a self,
            _input: UploadInput<'a>,
        ) -> BoxFuture<'a, Result<String, AssetError>> {
            Box::pin(async { Ok("https://cdn.example/x.png".to_string()) })
        }
    }

    #[test]
    fn media_kinds_bitset_and_labels() {
        let kinds = MediaKinds::IMAGE | MediaKinds::VIDEO;
        assert!(kinds.contains(MediaKind::Image));
        assert!(kinds.contains(MediaKind::Video));
        assert!(!kinds.contains(MediaKind::Audio));
        assert!(!MediaKinds::NONE.contains(MediaKind::Image));
        assert_eq!(MediaKind::Video.label(), "视频");
        assert_eq!(MediaKind::of_mime("video/mp4"), MediaKind::Video);
        assert_eq!(MediaKind::of_mime("audio/mpeg"), MediaKind::Audio);
        assert_eq!(MediaKind::of_mime("image/png"), MediaKind::Image);
        assert_eq!(MediaKinds::from(MediaKind::Audio).bits(), 4);
        assert_eq!(MediaKinds::from_bits(0xFF).bits(), 7);
    }

    #[test]
    fn default_provider_contract_is_image_only() {
        let provider: Arc<dyn Provider> = Arc::new(FixedProvider {
            name: "demo",
            kinds: MediaKinds::IMAGE,
            limit: 0,
        });
        assert_eq!(provider.kinds(), MediaKinds::IMAGE);
        assert_eq!(provider.max_size(), 0);
        assert!(!provider.session_scoped());
        assert!(!provider.skip_probe());
    }

    #[test]
    fn upload_input_carries_context() {
        let target = Target::new(aurorix_core::Scene::Group, "g1");
        let input = UploadInput::new(b"data", "a.png", "image/png")
            .with_target(target.clone())
            .with_trace("t1")
            .with_command("cmd");
        assert_eq!(input.size(), 4);
        assert_eq!(input.trace, "t1");
        assert_eq!(input.command, "cmd");
        assert_eq!(input.target, target);
        let ctx = UploadContext::new(target).with_trace("t").with_command("c");
        assert_eq!(ctx.trace, "t");
        assert_eq!(ctx.command, "c");
    }

    #[test]
    fn resolved_image_helpers() {
        let direct = ResolvedImage::passthrough("https://a/b.png");
        assert!(direct.is_passthrough());
        let uploaded = ResolvedImage::uploaded("https://cdn/x.png", "sgame", false).with_size(1, 2);
        assert!(!uploaded.is_passthrough());
        assert_eq!(uploaded.width, 1);
        assert_eq!(uploaded.height, 2);
    }
}
