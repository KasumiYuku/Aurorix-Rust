use aurorix_api::{ApiError, MediaUpload, MEDIA_FILE, MEDIA_VIDEO, MEDIA_VOICE};
use aurorix_assets::{decode, MediaKind, MediaSource};
use aurorix_plugin::Media;

pub fn media_kind(file_type: i32) -> Option<MediaKind> {
    match file_type {
        MEDIA_VIDEO => Some(MediaKind::Video),
        MEDIA_VOICE => Some(MediaKind::Audio),
        MEDIA_FILE => None,
        _ => Some(MediaKind::Image),
    }
}

pub fn media_name(file_type: i32, media: &Media) -> String {
    match file_type {
        MEDIA_VIDEO => String::from("video"),
        MEDIA_VOICE => String::from("voice"),
        MEDIA_FILE => String::from("file"),
        _ => match media {
            Media::Bytes { filename, .. } if !filename.is_empty() => filename.clone(),
            Media::Source(text) => {
                let trimmed = text.trim().trim_start_matches("file://");
                match trimmed.rsplit(['/', '\\']).next() {
                    Some(name) if !name.is_empty() && name != "." => name.to_string(),
                    _ => String::from("image"),
                }
            }
            Media::Bytes { .. } => String::from("image"),
        },
    }
}

pub fn upload_for(file_type: i32, media: &Media) -> Result<MediaUpload, ApiError> {
    let fallback = media_name(file_type, media);
    match media {
        Media::Bytes { data, filename } => {
            let name = match filename.is_empty() {
                true => fallback,
                false => filename.clone(),
            };
            Ok(MediaUpload::bytes(file_type, data.clone(), name))
        }
        Media::Source(text) => {
            let input = decode(MediaSource::Str(text)).map_err(|error| ApiError::Upload(error.to_string()))?;
            if !input.url.is_empty() {
                let mut upload = MediaUpload::from_url(input.url, file_type);
                upload.filename = fallback;
                return Ok(upload);
            }
            let name = match input.filename.is_empty() {
                true => fallback,
                false => input.filename,
            };
            Ok(MediaUpload::bytes(file_type, input.data, name))
        }
    }
}
