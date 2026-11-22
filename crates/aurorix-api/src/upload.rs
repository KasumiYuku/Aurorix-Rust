use std::time::Duration;

use aurorix_core::{Scene, Target};
use base64::Engine;
use md5::{Digest, Md5};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use sha1::Sha1;

use crate::api::BotApi;
use crate::error::ApiError;
use crate::transport::parse_business_error;

pub const DEFAULT_UPLOAD_THRESHOLD: usize = 3 << 20;
pub const MD5_HEAD_SIZE: usize = 10_002_432;
pub const PART_ATTEMPTS: u32 = 3;
pub const PART_RETRY_DELAY: Duration = Duration::from_millis(800);
pub const MIN_BUDGET: Duration = Duration::from_secs(60);
pub const MAX_BUDGET: Duration = Duration::from_secs(600);
pub const UPLOAD_RATE_BYTES: usize = 256 << 10;
pub const PART_RATE_BYTES: usize = 128 << 10;
pub const FALLBACK_BLOCK_SIZE: usize = 1024 * 1024;
pub const MEDIA_IMAGE: i32 = 1;
pub const MEDIA_VIDEO: i32 = 2;
pub const MEDIA_VOICE: i32 = 3;
pub const MEDIA_FILE: i32 = 4;

#[derive(Debug, Clone)]
pub struct MediaUpload {
    pub file_type: i32,
    pub data: Vec<u8>,
    pub filename: String,
    pub mime_type: String,
    pub url: Option<String>,
    pub force_chunked: bool,
}

impl MediaUpload {
    pub fn image(data: Vec<u8>, filename: impl Into<String>) -> Self {
        Self {
            file_type: MEDIA_IMAGE,
            data,
            filename: filename.into(),
            mime_type: String::new(),
            url: None,
            force_chunked: false,
        }
    }

    pub fn from_url(url: impl Into<String>, file_type: i32) -> Self {
        Self {
            file_type,
            data: Vec::new(),
            filename: String::new(),
            mime_type: String::new(),
            url: Some(url.into()),
            force_chunked: false,
        }
    }

    pub fn bytes(file_type: i32, data: Vec<u8>, filename: impl Into<String>) -> Self {
        Self {
            file_type,
            data,
            filename: filename.into(),
            mime_type: String::new(),
            url: None,
            force_chunked: false,
        }
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = mime_type.into();
        self
    }

    pub fn force_chunked(mut self) -> Self {
        self.force_chunked = true;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaResult {
    pub file_info: String,
    pub file_uuid: String,
    pub ttl: i64,
    pub raw_url: String,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    #[serde(default)]
    file_info: String,
    #[serde(default)]
    file_uuid: String,
    #[serde(default)]
    ttl: i64,
    #[serde(default)]
    raw_url: String,
}

#[derive(Debug, Deserialize)]
struct PrepareResponse {
    #[serde(default)]
    upload_id: String,
    #[serde(default)]
    block_size: String,
    #[serde(default)]
    parts: Vec<PreparePart>,
}

#[derive(Debug, Deserialize)]
struct PreparePart {
    #[serde(default)]
    index: i32,
    #[serde(default)]
    presigned_url: String,
}

pub fn md5_hex(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

pub fn head_bytes(data: &[u8], limit: usize) -> &[u8] {
    if data.len() <= limit {
        data
    } else {
        &data[..limit]
    }
}

pub fn md5_10m_hex(data: &[u8]) -> String {
    md5_hex(head_bytes(data, MD5_HEAD_SIZE))
}

pub fn size_budget(size: usize, rate: usize) -> Duration {
    let floor = MIN_BUDGET;
    let extra = Duration::from_secs((size / rate) as u64);
    let budget = floor + extra;
    if budget > MAX_BUDGET {
        MAX_BUDGET
    } else {
        budget
    }
}

pub fn upload_budget(size: usize) -> Duration {
    size_budget(size, UPLOAD_RATE_BYTES)
}

pub fn part_timeout(size: usize) -> Duration {
    size_budget(size, PART_RATE_BYTES)
}

pub fn media_file_name(file_type: i32, name: &str) -> String {
    if name.is_empty() || has_extension(name) {
        return name.to_string();
    }
    match file_type {
        MEDIA_VIDEO => format!("{name}.mp4"),
        MEDIA_VOICE => format!("{name}.mp3"),
        _ => name.to_string(),
    }
}

fn has_extension(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && !ext.is_empty(),
        None => false,
    }
}

pub fn media_base(target: &Target) -> Result<String, ApiError> {
    match target.scene {
        Scene::Group => Ok(format!("/v2/groups/{}", target.id)),
        Scene::C2c => Ok(format!("/v2/users/{}", target.id)),
        other => Err(ApiError::Unsupported {
            scene: other,
            what: "富媒体上传",
        }),
    }
}

impl BotApi {
    pub async fn upload(
        &self,
        target: &Target,
        upload: &MediaUpload,
    ) -> Result<MediaResult, ApiError> {
        if target.empty() {
            return Err(ApiError::InvalidTarget("会话目标为空"));
        }
        let base = media_base(target)?;
        let chunked = upload.url.is_none()
            && !upload.data.is_empty()
            && (upload.force_chunked || upload.data.len() > self.upload_threshold());
        if chunked {
            return self.chunked_upload(&base, upload).await;
        }
        self.simple_upload(&base, upload).await
    }

    pub async fn upload_image(
        &self,
        target: &Target,
        data: Vec<u8>,
        filename: impl Into<String>,
    ) -> Result<MediaResult, ApiError> {
        let upload = MediaUpload::image(data, filename);
        self.upload(target, &upload).await
    }

    pub async fn upload_media(
        &self,
        target: &Target,
        upload: &MediaUpload,
    ) -> Result<MediaResult, ApiError> {
        self.upload(target, upload).await
    }

    pub async fn upload_official_image(
        &self,
        target: &Target,
        data: Vec<u8>,
        filename: &str,
        mime_type: &str,
    ) -> Result<String, ApiError> {
        if target.scene.guild() {
            return Err(ApiError::Unsupported {
                scene: target.scene,
                what: "官方图床",
            });
        }
        if data.is_empty() {
            return Err(ApiError::upload("官方图床需要图片字节"));
        }
        let mut upload = MediaUpload::image(data, filename);
        upload.mime_type = mime_type.to_string();
        upload.force_chunked = true;
        let result = self.upload(target, &upload).await?;
        if result.raw_url.is_empty() {
            return Err(ApiError::upload("官方上传未返回 raw_url"));
        }
        Ok(result.raw_url)
    }

    async fn simple_upload(
        &self,
        base: &str,
        upload: &MediaUpload,
    ) -> Result<MediaResult, ApiError> {
        let mut body = serde_json::Map::new();
        body.insert(String::from("file_type"), json!(upload.file_type));
        body.insert(String::from("srv_send_msg"), json!(false));
        if let Some(url) = &upload.url {
            if url.is_empty() {
                return Err(ApiError::upload("上传既没有 URL 也没有字节"));
            }
            body.insert(String::from("url"), json!(url));
        } else if !upload.data.is_empty() {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&upload.data);
            body.insert(String::from("file_data"), json!(encoded));
        } else {
            return Err(ApiError::upload("上传既没有 URL 也没有字节"));
        }
        if !upload.filename.is_empty() {
            body.insert(
                String::from("file_name"),
                json!(media_file_name(upload.file_type, &upload.filename)),
            );
        }
        let endpoint = self.endpoint(&format!("{base}/files"));
        let raw = self
            .do_json_within(
                Method::POST,
                &endpoint,
                &serde_json::to_vec(&serde_json::Value::Object(body))?,
                upload_budget(upload.data.len()),
            )
            .await?;
        let parsed: UploadResponse = decode(&raw)?;
        if parsed.file_info.is_empty() {
            return Err(ApiError::upload("上传回执缺少 file_info"));
        }
        Ok(MediaResult {
            file_info: parsed.file_info,
            file_uuid: parsed.file_uuid,
            ttl: parsed.ttl,
            raw_url: parsed.raw_url,
        })
    }

    async fn chunked_upload(
        &self,
        base: &str,
        upload: &MediaUpload,
    ) -> Result<MediaResult, ApiError> {
        let prepare_endpoint = self.endpoint(&format!("{base}/upload_prepare"));
        let finish_endpoint = self.endpoint(&format!("{base}/upload_part_finish"));
        let files_endpoint = self.endpoint(&format!("{base}/files"));
        let file_name = media_file_name(upload.file_type, &upload.filename);
        let prepare_body = json!({
            "file_type": upload.file_type,
            "file_name": file_name,
            "file_size": upload.data.len(),
            "md5": md5_hex(&upload.data),
            "sha1": sha1_hex(&upload.data),
            "md5_10m": md5_10m_hex(&upload.data),
        });
        let raw = self
            .do_json_within(
                Method::POST,
                &prepare_endpoint,
                &serde_json::to_vec(&prepare_body)?,
                upload_budget(upload.data.len()),
            )
            .await?;
        let prepare: PrepareResponse = decode(&raw)?;
        let block_size = match prepare.block_size.trim().parse::<usize>() {
            Ok(size) if size > 0 => size,
            _ => FALLBACK_BLOCK_SIZE,
        };
        for part in &prepare.parts {
            let start = (part.index.max(1) as usize - 1).saturating_mul(block_size);
            if start >= upload.data.len() {
                continue;
            }
            let end = (start + block_size).min(upload.data.len());
            let chunk = &upload.data[start..end];
            let mut last: Option<ApiError> = None;
            for attempt in 1..=PART_ATTEMPTS {
                let outcome = self
                    .upload_part(
                        &part.presigned_url,
                        &finish_endpoint,
                        &prepare.upload_id,
                        part.index,
                        chunk,
                        &upload.mime_type,
                    )
                    .await;
                match outcome {
                    Ok(()) => {
                        last = None;
                        break;
                    }
                    Err(err) => {
                        last = Some(err);
                        if attempt < PART_ATTEMPTS {
                            tokio::time::sleep(PART_RETRY_DELAY * attempt).await;
                        }
                    }
                }
            }
            if let Some(err) = last {
                return Err(ApiError::upload(format!(
                    "分片 {} 重试 {PART_ATTEMPTS} 次仍失败: {err}",
                    part.index
                )));
            }
        }
        let files_body = json!({
            "upload_id": prepare.upload_id,
            "file_type": upload.file_type,
            "file_name": file_name,
            "srv_send_msg": false,
        });
        let raw = self
            .do_json_within(
                Method::POST,
                &files_endpoint,
                &serde_json::to_vec(&files_body)?,
                upload_budget(upload.data.len()),
            )
            .await?;
        let parsed: UploadResponse = decode(&raw)?;
        if parsed.file_info.is_empty() {
            return Err(ApiError::upload("分片上传回执缺少 file_info"));
        }
        Ok(MediaResult {
            file_info: parsed.file_info,
            file_uuid: parsed.file_uuid,
            ttl: parsed.ttl,
            raw_url: parsed.raw_url,
        })
    }

    async fn upload_part(
        &self,
        presigned_url: &str,
        finish_endpoint: &str,
        upload_id: &str,
        index: i32,
        chunk: &[u8],
        mime_type: &str,
    ) -> Result<(), ApiError> {
        self.put_presigned(presigned_url, chunk, mime_type, part_timeout(chunk.len()))
            .await?;
        let body = json!({
            "upload_id": upload_id,
            "part_index": index,
            "block_size": chunk.len(),
            "md5": md5_hex(chunk),
        });
        let raw = self
            .do_json_within(
                Method::POST,
                finish_endpoint,
                &serde_json::to_vec(&body)?,
                upload_budget(chunk.len()),
            )
            .await?;
        if let Some(err) = parse_business_error(&raw) {
            return Err(ApiError::Platform(err));
        }
        Ok(())
    }
}

fn decode<T: for<'de> Deserialize<'de>>(raw: &[u8]) -> Result<T, ApiError> {
    if let Some(err) = parse_business_error(raw) {
        return Err(ApiError::Platform(err));
    }
    Ok(serde_json::from_slice(raw)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_and_head_size_match_go() {
        assert_eq!(DEFAULT_UPLOAD_THRESHOLD, 3_145_728);
        assert_eq!(DEFAULT_UPLOAD_THRESHOLD, 3 << 20);
        assert_eq!(MD5_HEAD_SIZE, 10_002_432);
        assert_eq!(PART_ATTEMPTS, 3);
        assert_eq!(PART_RETRY_DELAY, Duration::from_millis(800));
        assert_eq!(MEDIA_IMAGE, 1);
        assert_eq!(MEDIA_VIDEO, 2);
        assert_eq!(MEDIA_VOICE, 3);
        assert_eq!(MEDIA_FILE, 4);
    }

    #[test]
    fn md5_of_fixed_bytes_is_stable() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(b"The quick brown fox jumps over the lazy dog"),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
        assert_eq!(md5_10m_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn md5_10m_only_covers_the_head() {
        let mut data = vec![0u8; MD5_HEAD_SIZE + 4096];
        for (index, slot) in data.iter_mut().enumerate() {
            *slot = (index % 251) as u8;
        }
        assert_eq!(head_bytes(&data, MD5_HEAD_SIZE).len(), MD5_HEAD_SIZE);
        assert_eq!(md5_10m_hex(&data), md5_hex(&data[..MD5_HEAD_SIZE]));
        assert_ne!(md5_10m_hex(&data), md5_hex(&data), "尾部字节不该进 md5_10m");
        let short = vec![7u8; 16];
        assert_eq!(head_bytes(&short, MD5_HEAD_SIZE), &short[..]);
        assert_eq!(md5_10m_hex(&short), md5_hex(&short));
    }

    #[test]
    fn upload_budget_scales_with_size() {
        assert_eq!(upload_budget(0), Duration::from_secs(60));
        assert_eq!(upload_budget(256 << 10), Duration::from_secs(61));
        assert_eq!(upload_budget(3 << 20), Duration::from_secs(72));
        assert_eq!(
            upload_budget(1 << 30),
            Duration::from_secs(600),
            "上限 10 分钟"
        );
        assert_eq!(part_timeout(0), Duration::from_secs(60));
        assert_eq!(part_timeout(128 << 10), Duration::from_secs(61));
        assert_eq!(part_timeout(1 << 30), Duration::from_secs(600));
        assert!(
            part_timeout(4 << 20) > upload_budget(4 << 20),
            "分片超时更宽松"
        );
    }

    #[test]
    fn file_names_get_default_extension() {
        assert_eq!(media_file_name(MEDIA_VIDEO, "clip"), "clip.mp4");
        assert_eq!(media_file_name(MEDIA_VOICE, "voice"), "voice.mp3");
        assert_eq!(media_file_name(MEDIA_IMAGE, "pic"), "pic");
        assert_eq!(media_file_name(MEDIA_IMAGE, "pic.png"), "pic.png");
        assert_eq!(media_file_name(MEDIA_IMAGE, ""), "");
    }

    #[test]
    fn media_base_covers_group_and_c2c_only() {
        assert_eq!(
            media_base(&Target::new(Scene::Group, "g1")).unwrap(),
            "/v2/groups/g1"
        );
        assert_eq!(
            media_base(&Target::new(Scene::C2c, "u1")).unwrap(),
            "/v2/users/u1"
        );
        assert!(media_base(&Target::new(Scene::Channel, "c1")).is_err());
    }

    #[test]
    fn upload_builders_default_to_image() {
        let upload = MediaUpload::image(vec![1, 2, 3], "a.png");
        assert_eq!(upload.file_type, MEDIA_IMAGE);
        assert!(!upload.force_chunked);
        assert!(upload.url.is_none());
        let forced = upload.force_chunked().with_mime_type("image/png");
        assert!(forced.force_chunked);
        assert_eq!(forced.mime_type, "image/png");
        let by_url = MediaUpload::from_url("https://x/y.png", MEDIA_IMAGE);
        assert_eq!(by_url.url.as_deref(), Some("https://x/y.png"));
        assert!(by_url.data.is_empty());
    }
}
