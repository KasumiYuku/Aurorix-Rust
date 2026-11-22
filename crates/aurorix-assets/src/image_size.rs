use std::time::Duration;

use crate::http::ReqwestClient;

pub const DIMENSION_TIMEOUT_SECS: u64 = 5;

pub const MAX_PROBE_BYTES: usize = 1 << 20;

pub const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

pub fn probe(data: &[u8]) -> Option<(u32, u32)> {
    crate::mime::probe_size(data)
}

pub async fn probe_dimensions(url: &str) -> Option<(u32, u32)> {
    let client = ReqwestClient::new();
    let mut response = client
        .inner()
        .get(url)
        .header("User-Agent", CHROME_USER_AGENT)
        .timeout(Duration::from_secs(DIMENSION_TIMEOUT_SECS))
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::OK {
        return None;
    }
    let mut head: Vec<u8> = Vec::new();
    while head.len() < MAX_PROBE_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => head.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(_) => return None,
        }
    }
    head.truncate(MAX_PROBE_BYTES);
    probe(&head)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_HEADER: [u8; 26] = [
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x50, 0x08, 0x06,
    ];

    fn jpeg_header(width: u16, height: u16) -> Vec<u8> {
        let mut buf = vec![0xFF, 0xD8];
        buf.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        buf.extend_from_slice(b"JFIF\0");
        buf.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        buf.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        buf.extend_from_slice(&height.to_be_bytes());
        buf.extend_from_slice(&width.to_be_bytes());
        buf.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        buf
    }

    fn vp8l_webp(width: u32, height: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(b"WEBPVP8L");
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.push(0x2f);
        let bits = (width - 1) | ((height - 1) << 14);
        buf.extend_from_slice(&bits.to_le_bytes());
        buf.resize(32, 0);
        buf
    }

    #[test]
    fn png_dimensions_are_big_endian() {
        assert_eq!(probe(&PNG_HEADER), Some((256, 80)));
    }

    #[test]
    fn gif_dimensions_are_little_endian() {
        let mut buf = b"GIF89a".to_vec();
        buf.extend_from_slice(&[0x40, 0x01, 0xF0, 0x00]);
        buf.resize(12, 0);
        assert_eq!(probe(&buf), Some((320, 240)));
    }

    #[test]
    fn jpeg_dimensions_come_from_the_sof_segment() {
        assert_eq!(probe(&jpeg_header(1024, 768)), Some((1024, 768)));
        assert_eq!(probe(&jpeg_header(1, 1)), Some((1, 1)));
    }

    #[test]
    fn webp_dimensions_cover_all_three_chunks() {
        assert_eq!(probe(&vp8l_webp(300, 200)), Some((300, 200)));
        let mut lossy = Vec::new();
        lossy.extend_from_slice(b"RIFF");
        lossy.extend_from_slice(&0u32.to_le_bytes());
        lossy.extend_from_slice(b"WEBPVP8 ");
        lossy.extend_from_slice(&0u32.to_le_bytes());
        lossy.extend_from_slice(&[0x00, 0x00, 0x00, 0x9D, 0x01, 0x2A]);
        lossy.extend_from_slice(&640u16.to_le_bytes());
        lossy.extend_from_slice(&480u16.to_le_bytes());
        assert_eq!(probe(&lossy), Some((640, 480)));
        let mut extended = Vec::new();
        extended.extend_from_slice(b"RIFF");
        extended.extend_from_slice(&0u32.to_le_bytes());
        extended.extend_from_slice(b"WEBPVP8X");
        extended.extend_from_slice(&0u32.to_le_bytes());
        extended.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        extended.extend_from_slice(&[0x0F, 0x01, 0x00]);
        extended.extend_from_slice(&[0x2B, 0x01, 0x00]);
        assert_eq!(probe(&extended), Some((272, 300)));
    }

    #[test]
    fn unknown_and_truncated_buffers_yield_nothing() {
        assert_eq!(probe(b""), None);
        assert_eq!(probe(b"ab"), None);
        assert_eq!(probe(b"not an image at all"), None);
        assert_eq!(probe(&PNG_HEADER[..20]), None);
        assert_eq!(probe(&[0xFF, 0xD8, 0xFF, 0xC0, 0x00]), None);
        assert_eq!(probe(b"RIFF\0\0\0\0WEBP"), None);
    }

    #[test]
    fn bmp_and_video_containers_have_no_probe() {
        assert_eq!(probe(b"BM\0\0\0\0\0\0\0\0\0\0\0\0"), None);
        let mut mp4 = vec![0u8, 0, 0, 0x18];
        mp4.extend_from_slice(b"ftypisom\0\0\0\0\0\0\0\0");
        assert_eq!(probe(&mp4), None);
    }
}
