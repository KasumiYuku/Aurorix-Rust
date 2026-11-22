use std::sync::Arc;

use crate::error::AssetError;
use crate::official::{official_upload, OfficialUploadRequest};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};

pub const NAME: &str = "qqbot";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "官方富媒体（qqbot）",
        Vec::<ConfigField>::new(),
        Box::new(|_item, _http| Ok(Arc::new(QqbotProvider))),
    )
    .default_on()
    .probe_skip()
}

pub struct QqbotProvider;

impl Provider for QqbotProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn skip_probe(&self) -> bool {
        true
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let Some(upload) = official_upload() else {
                return Err(AssetError::message(
                    "官方上传未注入（当前实例不是官方机器人运行时）",
                ));
            };
            if input.target.scene.guild() {
                return Err(AssetError::message(
                    "官方图床只能在群聊/私聊里用, 频道里没有可用的上传目标",
                ));
            }
            if input.target.empty() {
                return Err(AssetError::message("缺少会话目标"));
            }
            let request = OfficialUploadRequest {
                bytes: input.bytes.to_vec(),
                filename: input.filename.clone(),
                mime: input.mime.clone(),
                target: input.target.clone(),
                trace: input.trace.clone(),
                command: input.command.clone(),
            };
            let url = upload(request).await?;
            if url.is_empty() {
                return Err(AssetError::message("官方上传未返回直链"));
            }
            Ok(url)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderItem;
    use crate::official::set_official_upload;
    use crate::testkit::ResultExt as _;
    use crate::testkit::{official_lock, MockHttp};
    use aurorix_core::{Scene, Target};

    fn provider() -> Arc<dyn Provider> {
        (entry().factory)(&ProviderItem::new(NAME), MockHttp::shared(vec![]))
            .expect("构造 qqbot 失败")
    }

    #[tokio::test]
    async fn delegates_to_the_injected_uploader() {
        let _guard = official_lock().await;
        let upload: crate::official::OfficialUploadFn = Arc::new(|request| {
            Box::pin(async move {
                assert_eq!(request.target.id, "g1");
                assert_eq!(request.trace, "trace-1");
                Ok(format!("https://raw.example/{}", request.filename))
            })
        });
        set_official_upload(Some(upload));
        let input = UploadInput::new(b"bytes", "a.png", "image/png")
            .with_target(Target::new(Scene::Group, "g1"))
            .with_trace("trace-1");
        let url = provider().upload(input).await.unwrap();
        assert_eq!(url, "https://raw.example/a.png");
        set_official_upload(None);
    }

    #[tokio::test]
    async fn reports_missing_targets_and_idle_uploader() {
        let _guard = official_lock().await;
        set_official_upload(None);
        let err = provider()
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert!(err.to_string().contains("官方上传未注入"));

        let upload: crate::official::OfficialUploadFn =
            Arc::new(|_request| Box::pin(async { Ok("https://raw.example/a.png".to_string()) }));
        set_official_upload(Some(upload));
        let guild = UploadInput::new(b"bytes", "a.png", "image/png")
            .with_target(Target::new(Scene::Channel, "c1"));
        let err = provider().upload(guild).await.failure();
        assert_eq!(
            err.to_string(),
            "官方图床只能在群聊/私聊里用, 频道里没有可用的上传目标"
        );
        let empty = UploadInput::new(b"bytes", "a.png", "image/png");
        let err = provider().upload(empty).await.failure();
        assert_eq!(err.to_string(), "缺少会话目标");
        set_official_upload(None);
    }

    #[tokio::test]
    async fn empty_official_url_is_rejected() {
        let _guard = official_lock().await;
        let upload: crate::official::OfficialUploadFn =
            Arc::new(|_request| Box::pin(async { Ok(String::new()) }));
        set_official_upload(Some(upload));
        let input = UploadInput::new(b"bytes", "a.png", "image/png")
            .with_target(Target::new(Scene::C2c, "u1"));
        let err = provider().upload(input).await.failure();
        assert_eq!(err.to_string(), "官方上传未返回直链");
        set_official_upload(None);
    }

    #[test]
    fn provider_is_session_scoped_and_skips_probe() {
        let provider = QqbotProvider;
        assert!(provider.session_scoped());
        assert!(provider.skip_probe());
        assert_eq!(provider.kinds(), MediaKinds::IMAGE);
        let entry = entry();
        assert!(entry.default_on);
        assert!(entry.probe_skip);
    }
}
