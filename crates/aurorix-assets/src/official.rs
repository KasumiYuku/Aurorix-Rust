use std::sync::Arc;

use aurorix_core::Target;
use parking_lot::RwLock;

use crate::error::AssetError;
use crate::http::BoxFuture;

#[derive(Clone, Debug)]
pub struct OfficialUploadRequest {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime: String,
    pub target: Target,
    pub trace: String,
    pub command: String,
}

pub type OfficialUploadFn = Arc<
    dyn Fn(OfficialUploadRequest) -> BoxFuture<'static, Result<String, AssetError>> + Send + Sync,
>;

static OFFICIAL_UPLOAD: RwLock<Option<OfficialUploadFn>> = RwLock::new(None);

pub fn set_official_upload(upload: Option<OfficialUploadFn>) {
    *OFFICIAL_UPLOAD.write() = upload;
}

pub fn official_upload() -> Option<OfficialUploadFn> {
    OFFICIAL_UPLOAD.read().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::official_lock;

    #[tokio::test]
    async fn injection_is_idempotent_and_readable() {
        let _guard = official_lock().await;
        assert!(official_upload().is_none());
        let upload: OfficialUploadFn = Arc::new(|request: OfficialUploadRequest| {
            Box::pin(async move {
                assert_eq!(request.filename, "a.png");
                Ok("https://raw.example/a.png".to_string())
            })
        });
        set_official_upload(Some(upload.clone()));
        let read_back = official_upload().expect("注入后应当可读");
        let request = OfficialUploadRequest {
            bytes: vec![1, 2, 3],
            filename: "a.png".to_string(),
            mime: "image/png".to_string(),
            target: Target::new(aurorix_core::Scene::Group, "g1"),
            trace: "t".to_string(),
            command: "c".to_string(),
        };
        assert_eq!(
            read_back(request).await.unwrap(),
            "https://raw.example/a.png"
        );
        set_official_upload(None);
        assert!(official_upload().is_none());
    }
}
