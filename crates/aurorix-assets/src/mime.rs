pub const PNG_SIGNATURE: [u8; 4] = [0x89, b'P', b'N', b'G'];

pub fn is_image(buf: &[u8]) -> bool {
    if buf.len() < 12 {
        return false;
    }
    if buf[0..4] == PNG_SIGNATURE {
        return true;
    }
    if buf[0] == 0xFF && buf[1] == 0xD8 {
        return true;
    }
    if buf[0] == b'G' && buf[1] == b'I' && buf[2] == b'F' && buf[3] == b'8' {
        return true;
    }
    if buf[0..4] == *b"RIFF" && buf[8..12] == *b"WEBP" {
        return true;
    }
    buf[0] == b'B' && buf[1] == b'M'
}

pub fn probe_image_mime(buf: &[u8]) -> &'static str {
    if buf.len() < 4 {
        return "";
    }
    if buf[0..4] == PNG_SIGNATURE {
        return "image/png";
    }
    if buf[0] == 0xFF && buf[1] == 0xD8 {
        return "image/jpeg";
    }
    if buf[0] == b'G' && buf[1] == b'I' && buf[2] == b'F' && buf[3] == b'8' {
        return "image/gif";
    }
    if buf[0..4] == *b"RIFF" && buf.len() >= 12 && buf[8..12] == *b"WEBP" {
        return "image/webp";
    }
    if buf[0] == b'B' && buf[1] == b'M' {
        return "image/bmp";
    }
    ""
}

pub fn probe_video_mime(buf: &[u8]) -> &'static str {
    if buf.len() >= 12 && &buf[4..8] == b"ftyp" {
        return match &buf[8..12] {
            b"qt  " => "video/quicktime",
            b"heic" | b"heix" | b"mif1" | b"msf1" => "image/heic",
            _ => "video/mp4",
        };
    }
    if buf.len() >= 4 && buf[0] == 0x1a && buf[1] == 0x45 && buf[2] == 0xdf && buf[3] == 0xa3 {
        return "video/webm";
    }
    ""
}

pub fn probe_media_mime(buf: &[u8]) -> &'static str {
    let image = probe_image_mime(buf);
    if !image.is_empty() {
        return image;
    }
    probe_video_mime(buf)
}

pub fn ext_from_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "",
    }
}

pub fn media_ext(mime: &str) -> &'static str {
    let ext = ext_from_mime(mime);
    if !ext.is_empty() {
        return ext;
    }
    match mime {
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        _ => "",
    }
}

pub fn probe_size(buf: &[u8]) -> Option<(u32, u32)> {
    if buf.len() < 4 {
        return None;
    }
    let n = buf.len();
    if n >= 24 && buf[0..4] == PNG_SIGNATURE {
        return Some((be_u32(&buf[16..20]), be_u32(&buf[20..24])));
    }
    if n >= 10 && buf[0] == b'G' && buf[1] == b'I' && buf[2] == b'F' {
        return Some((
            u32::from(le_u16(&buf[6..8])),
            u32::from(le_u16(&buf[8..10])),
        ));
    }
    if buf[0] == 0xFF && buf[1] == 0xD8 {
        let mut off = 2usize;
        while off + 9 < n {
            if buf[off] != 0xFF {
                off += 1;
                continue;
            }
            let marker = buf[off + 1];
            if marker == 0xD8 || marker == 0xD9 || marker == 0x01 || (0xD0..=0xD7).contains(&marker)
            {
                off += 2;
                continue;
            }
            let seg_len = usize::from(be_u16(&buf[off + 2..off + 4]));
            if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 {
                return Some((
                    u32::from(be_u16(&buf[off + 7..off + 9])),
                    u32::from(be_u16(&buf[off + 5..off + 7])),
                ));
            }
            off += 2 + seg_len;
        }
        return None;
    }
    if n >= 30 && buf[0..4] == *b"RIFF" && buf[8..12] == *b"WEBP" {
        match &buf[12..16] {
            b"VP8 " => {
                return Some((
                    u32::from(le_u16(&buf[26..28]) & 0x3FFF),
                    u32::from(le_u16(&buf[28..30]) & 0x3FFF),
                ));
            }
            b"VP8L" => {
                if n >= 25 {
                    let bits = le_u32(&buf[21..25]);
                    return Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1));
                }
            }
            b"VP8X" => {
                let width = u32::from(buf[24]) | u32::from(buf[25]) << 8 | u32::from(buf[26]) << 16;
                let height =
                    u32::from(buf[27]) | u32::from(buf[28]) << 8 | u32::from(buf[29]) << 16;
                return Some((width + 1, height + 1));
            }
            _ => {}
        }
    }
    None
}

fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub fn content_type_matches(kind: crate::provider::MediaKind, content_type: &str) -> bool {
    let trimmed = content_type.trim().to_lowercase();
    let base = trimmed
        .split_once(';')
        .map_or(trimmed.as_str(), |(head, _)| head.trim());
    match kind {
        crate::provider::MediaKind::Image => base.starts_with("image/"),
        crate::provider::MediaKind::Video => base.starts_with("video/"),
        crate::provider::MediaKind::Audio => base.starts_with("audio/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MediaKind;

    const PNG_HEADER: [u8; 26] = [
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x50, 0x08, 0x06,
    ];

    #[test]
    fn png_size_and_mime_detected() {
        assert!(is_image(&PNG_HEADER));
        assert_eq!(probe_image_mime(&PNG_HEADER), "image/png");
        assert_eq!(probe_size(&PNG_HEADER), Some((256, 80)));
    }

    #[test]
    fn short_buffers_are_not_images() {
        assert!(!is_image(b"garbage"));
        assert_eq!(probe_image_mime(b"ab"), "");
        assert_eq!(probe_size(b"ab"), None);
    }

    #[test]
    fn gif_size_is_little_endian() {
        let mut buf = b"GIF89a".to_vec();
        buf.extend_from_slice(&[0x40, 0x01, 0xF0, 0x00]);
        buf.resize(12, 0);
        assert_eq!(probe_size(&buf), Some((320, 240)));
    }

    #[test]
    fn video_containers_are_recognised() {
        let mut mp4 = vec![0u8, 0, 0, 0x18];
        mp4.extend_from_slice(b"ftypisom");
        assert_eq!(probe_media_mime(&mp4), "video/mp4");
        let webm = [0x1a, 0x45, 0xdf, 0xa3, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(probe_video_mime(&webm), "video/webm");
        assert_eq!(media_ext("video/webm"), "webm");
        assert_eq!(media_ext("audio/mpeg"), "mp3");
    }

    #[test]
    fn content_type_matching_ignores_parameters() {
        assert!(content_type_matches(
            MediaKind::Image,
            "image/png; charset=utf-8"
        ));
        assert!(!content_type_matches(MediaKind::Image, "text/html"));
        assert!(content_type_matches(MediaKind::Video, "VIDEO/MP4"));
        assert!(content_type_matches(MediaKind::Audio, "audio/mpeg"));
        assert!(!content_type_matches(MediaKind::Audio, "application/zip"));
    }
}
