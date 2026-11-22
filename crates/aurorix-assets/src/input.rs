use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;

use crate::error::AssetError;
use crate::mime::{is_image, probe_image_mime};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaInput {
    pub data: Vec<u8>,
    pub url: String,
    pub filename: String,
    pub mime: String,
}

#[derive(Clone, Copy, Debug)]
pub enum MediaSource<'a> {
    Str(&'a str),
    Bytes(&'a [u8]),
}

impl<'a> From<&'a str> for MediaSource<'a> {
    fn from(value: &'a str) -> Self {
        MediaSource::Str(value)
    }
}

impl<'a> From<&'a [u8]> for MediaSource<'a> {
    fn from(value: &'a [u8]) -> Self {
        MediaSource::Bytes(value)
    }
}

pub fn decode(source: MediaSource<'_>) -> Result<MediaInput, AssetError> {
    match source {
        MediaSource::Bytes(bytes) => {
            if bytes.is_empty() {
                return Err(AssetError::message("空媒体数据"));
            }
            Ok(MediaInput {
                data: bytes.to_vec(),
                filename: "inline".to_string(),
                ..MediaInput::default()
            })
        }
        MediaSource::Str(text) => decode_str(text, false),
    }
}

pub fn decode_image(source: MediaSource<'_>) -> Result<MediaInput, AssetError> {
    match source {
        MediaSource::Bytes(bytes) => {
            if !is_image(bytes) {
                return Err(AssetError::message("字节非图片"));
            }
            Ok(MediaInput {
                data: bytes.to_vec(),
                filename: "inline".to_string(),
                ..MediaInput::default()
            })
        }
        MediaSource::Str(text) => decode_str(text, true),
    }
}

fn decode_str(src: &str, want_image: bool) -> Result<MediaInput, AssetError> {
    let trimmed = src.trim();
    if trimmed.starts_with("data:") {
        return decode_data_url(trimmed, want_image);
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(MediaInput {
            url: trimmed.to_string(),
            filename: base_name(trimmed),
            ..MediaInput::default()
        });
    }
    let path = trimmed.strip_prefix("file://").unwrap_or(trimmed);
    if let Some(data) = decode_base64_lenient(path, want_image) {
        return Ok(MediaInput {
            data,
            filename: "inline".to_string(),
            ..MediaInput::default()
        });
    }
    let data = std::fs::read(path)?;
    if want_image && !is_image(&data) {
        return Err(AssetError::message("文件非图片"));
    }
    Ok(MediaInput {
        data,
        filename: base_name(path),
        ..MediaInput::default()
    })
}

fn decode_data_url(src: &str, want_image: bool) -> Result<MediaInput, AssetError> {
    let Some((meta, payload)) = src.split_once(',') else {
        return Err(AssetError::message("非法 data URL"));
    };
    if payload.is_empty() {
        return Err(AssetError::message("非法 data URL"));
    }
    let meta = meta.strip_prefix("data:").unwrap_or(meta);
    let mime = meta.split_once(';').map_or(meta, |(head, _)| head);
    if meta.contains(";base64") {
        let Some(data) = decode_base64_lenient(payload, want_image) else {
            return Err(AssetError::message("data URL base64 解码失败"));
        };
        return Ok(MediaInput {
            data,
            filename: "inline".to_string(),
            mime: mime.to_string(),
            ..MediaInput::default()
        });
    }
    Ok(MediaInput {
        data: payload.as_bytes().to_vec(),
        filename: "inline".to_string(),
        mime: mime.to_string(),
        ..MediaInput::default()
    })
}

fn decode_base64_lenient(value: &str, want_image: bool) -> Option<Vec<u8>> {
    let value = value.strip_prefix("base64,").unwrap_or(value);
    let cleaned: String = value
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '\t' | '\n' | '\r'))
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let acceptable = |data: &[u8]| {
        if want_image {
            is_image(data)
        } else {
            !data.is_empty()
        }
    };
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        if let Ok(data) = engine.decode(cleaned.as_str()) {
            if acceptable(&data) {
                return Some(data);
            }
        }
    }
    let padding = (4 - cleaned.len() % 4) % 4;
    if padding > 0 {
        let mut padded = cleaned.clone();
        padded.extend(std::iter::repeat_n('=', padding));
        if let Ok(data) = STANDARD.decode(padded.as_str()) {
            if acceptable(&data) {
                return Some(data);
            }
        }
    }
    None
}

pub fn has_ext(name: &str) -> bool {
    match (name.rfind('.'), name.rfind('/')) {
        (Some(dot), Some(slash)) => dot > slash,
        (Some(_), None) => true,
        _ => false,
    }
}

pub fn base_name(path: &str) -> String {
    let trimmed = path.split(['?', '#']).next().unwrap_or(path);
    match trimmed.rsplit_once('/') {
        Some((_, tail)) if !tail.is_empty() => tail.to_string(),
        Some(_) => path.to_string(),
        None => trimmed.to_string(),
    }
}

pub fn filename_for(mime: &str, prefix: &str) -> String {
    let ext = crate::mime::ext_from_mime(mime);
    if ext.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}.{ext}")
    }
}

pub fn probe_mime_of(data: &[u8]) -> String {
    probe_image_mime(data).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mime::PNG_SIGNATURE;

    fn png() -> Vec<u8> {
        let mut data = PNG_SIGNATURE.to_vec();
        data.extend_from_slice(&[
            0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R',
        ]);
        data.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 3]);
        data
    }

    #[test]
    fn decodes_raw_bytes_and_image_bytes() {
        let input = decode(MediaSource::Bytes(b"hello")).unwrap();
        assert_eq!(input.data, b"hello");
        assert_eq!(input.filename, "inline");
        assert!(decode(MediaSource::Bytes(b"")).is_err());
        let image = decode_image(MediaSource::Bytes(&png())).unwrap();
        assert!(image.data.starts_with(&PNG_SIGNATURE));
        assert!(decode_image(MediaSource::Bytes(b"not an image")).is_err());
    }

    #[test]
    fn decodes_base64_data_url() {
        let raw = crate::sign::base64_encode(&png());
        let url = format!("data:image/png;base64,{raw}");
        let input = decode_image(MediaSource::Str(&url)).unwrap();
        assert_eq!(input.mime, "image/png");
        assert_eq!(input.data, png());
        assert_eq!(input.filename, "inline");
        let plain = "data:text/plain,hello%20world";
        let input = decode(MediaSource::Str(plain)).unwrap();
        assert_eq!(input.mime, "text/plain");
        assert_eq!(input.data, b"hello%20world");
        assert!(decode(MediaSource::Str("data:")).is_err());
    }

    #[test]
    fn decodes_bare_base64_with_and_without_padding() {
        let raw = crate::sign::base64_encode(&png());
        let input = decode_image(MediaSource::Str(&raw)).unwrap();
        assert_eq!(input.data, png());
        let trimmed = raw.trim_end_matches('=').to_string();
        let input = decode_image(MediaSource::Str(&trimmed)).unwrap();
        assert_eq!(input.data, png());
        let url_safe = URL_SAFE.encode(png());
        assert_eq!(
            decode_image(MediaSource::Str(&url_safe)).unwrap().data,
            png()
        );
    }

    #[test]
    fn rejects_non_image_files_for_image_decode() {
        let dir = std::env::temp_dir().join("aurorix-assets-input-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, b"plain text").unwrap();
        assert!(decode(MediaSource::Str(path.to_str().unwrap())).is_ok());
        assert!(decode_image(MediaSource::Str(path.to_str().unwrap())).is_err());
        let missing = dir.join("missing.bin");
        assert!(decode(MediaSource::Str(missing.to_str().unwrap())).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn public_urls_are_kept_as_urls() {
        let input = decode(MediaSource::Str("https://cdn.example.com/a/b.png?x=1")).unwrap();
        assert_eq!(input.url, "https://cdn.example.com/a/b.png?x=1");
        assert_eq!(input.filename, "b.png");
        assert!(input.data.is_empty());
    }

    #[test]
    fn extension_helpers_match_go() {
        assert!(has_ext("a.png"));
        assert!(!has_ext("a"));
        assert!(!has_ext("dir.d/a"));
        assert_eq!(base_name("/tmp/x/图片.png"), "图片.png");
        assert_eq!(filename_for("image/png", "inline"), "inline.png");
        assert_eq!(filename_for("application/zip", "inline"), "inline");
    }
}
