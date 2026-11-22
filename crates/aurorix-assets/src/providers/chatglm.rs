use std::sync::Arc;

use serde::Deserialize;

use crate::error::AssetError;
use crate::http::{call, parse_json, HttpClient, HttpMethod, HttpRequest, Multipart};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};

pub const NAME: &str = "chatglm";
const ENDPOINT: &str = "https://chatglm.cn/chatglm/backend-api/assistant/file_upload";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "智谱 chatglm",
        Vec::<ConfigField>::new(),
        Box::new(|_item, http| Ok(Arc::new(ChatglmProvider { http }))),
    )
}

pub struct ChatglmProvider {
    http: Arc<dyn HttpClient>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatglmEnvelope {
    #[serde(default)]
    result: ChatglmResult,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Default, Deserialize)]
struct ChatglmResult {
    #[serde(default)]
    file_url: String,
}

impl Provider for ChatglmProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let form = Multipart::new().file(
                "file",
                input.filename.clone(),
                input.mime.clone(),
                input.bytes.to_vec(),
            );
            let request = HttpRequest::new(HttpMethod::Post, ENDPOINT)
                .header("User-Agent", USER_AGENT)
                .multipart(form);
            let response = call(self.http.as_ref(), request).await?;
            let parsed: ChatglmEnvelope = parse_json(&response)?;
            if parsed.result.file_url.is_empty() {
                return Err(AssetError::message(format!("chatglm: {}", parsed.message)));
            }
            Ok(parsed.result.file_url)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderItem;
    use crate::http::HttpResponse;
    use crate::testkit::MockHttp;
    use crate::testkit::ResultExt as _;

    fn provider(mock: Arc<MockHttp>) -> Arc<dyn Provider> {
        (entry().factory)(&ProviderItem::new(NAME), mock).expect("构造 chatglm 失败")
    }

    #[tokio::test]
    async fn uploads_single_file_part() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"result":{"file_url":"https://chatglm.example/a.png","file_name":"a.png"},"message":"ok"}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://chatglm.example/a.png");
        let request = mock.last();
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.url, ENDPOINT);
        assert!(request
            .header_value("User-Agent")
            .unwrap()
            .contains("Chrome/120"));
        let form = request.multipart_form().expect("应当是 multipart");
        assert_eq!(form.file_part("file").unwrap().filename, "a.png");
        assert_eq!(form.file_part("file").unwrap().data, b"bytes");
    }

    #[tokio::test]
    async fn missing_file_url_reports_message() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"result":{"file_url":""},"message":"文件过大"}"#,
        )]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "chatglm: 文件过大");
    }

    #[test]
    fn has_no_config_fields() {
        assert!(entry().fields.is_empty());
    }
}
