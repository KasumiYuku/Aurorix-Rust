pub const DEFAULT_MAINTENANCE_MESSAGE: &str = "当前该功能模块正在维护中，请等待维护完成再重试";

pub const DEFAULT_DENIED_MESSAGE: &str = "你没有权限使用该指令";

pub const DEFAULT_UPLOAD_THRESHOLD: usize = 3 << 20;

pub const DEFAULT_IMAGE_MAX_WIDTH: u32 = 350;

pub const DEFAULT_IMAGE_MAX_HEIGHT: u32 = 438;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    PlainText = 0,
    Markdown = 2,
    Media = 7,
}

impl MessageType {
    pub const fn code(self) -> u8 {
        self as u8
    }
}

pub mod media_kind {
    pub const IMAGE: u8 = 1;
    pub const VIDEO: u8 = 2;
    pub const VOICE: u8 = 3;
    pub const FILE: u8 = 4;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_type_codes_match_wire_values() {
        assert_eq!(MessageType::PlainText.code(), 0);
        assert_eq!(MessageType::Markdown.code(), 2);
        assert_eq!(MessageType::Media.code(), 7);
    }

    #[test]
    fn media_kinds_match_wire_values() {
        assert_eq!(media_kind::IMAGE, 1);
        assert_eq!(media_kind::VIDEO, 2);
        assert_eq!(media_kind::VOICE, 3);
        assert_eq!(media_kind::FILE, 4);
    }

    #[test]
    fn threshold_is_three_megabytes() {
        assert_eq!(DEFAULT_UPLOAD_THRESHOLD, 3_145_728);
    }

    #[test]
    fn image_limits_match_go_constants() {
        assert_eq!(DEFAULT_IMAGE_MAX_WIDTH, 350);
        assert_eq!(DEFAULT_IMAGE_MAX_HEIGHT, 438);
    }
}
