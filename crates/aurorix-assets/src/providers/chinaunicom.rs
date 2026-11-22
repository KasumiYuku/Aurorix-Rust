use std::sync::Arc;

use serde::Deserialize;

use crate::error::AssetError;
use crate::http::{call, parse_json, HttpClient, HttpMethod, HttpRequest, Multipart};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::uuid_hex;

use super::util::{cfg_str, err_no_data};

pub const NAME: &str = "chinaunicom";
const ENDPOINT: &str = "https://iotpservice.smartont.net/api-user/upload.do";
const DEFAULT_VERSION: &str = "7.8.2";
const DEFAULT_CHANNEL: &str = "oppo_woapp";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "联通 App",
        vec![
            ConfigField::text("appVersion", "客户端版本").default_value(DEFAULT_VERSION),
            ConfigField::text("appChannel", "渠道").default_value(DEFAULT_CHANNEL),
        ],
        Box::new(|item, http| {
            Ok(Arc::new(UnicomProvider {
                http,
                app_version: cfg_str(&item.config, "appVersion", DEFAULT_VERSION),
                app_channel: cfg_str(&item.config, "appChannel", DEFAULT_CHANNEL),
            }))
        }),
    )
}

pub struct UnicomProvider {
    http: Arc<dyn HttpClient>,
    app_version: String,
    app_channel: String,
}

#[derive(Debug, Default, Deserialize)]
struct UnicomEnvelope {
    #[serde(default, rename = "RSP")]
    rsp: UnicomRsp,
}

#[derive(Debug, Default, Deserialize)]
struct UnicomRsp {
    #[serde(default, rename = "DATA")]
    data: String,
}

impl Provider for UnicomProvider {
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
                .header("User-Agent", "okhttp/4.9.0")
                .header("Connection", "keep-alive")
                .header("Accept", "*/*")
                .header("clientsign", "MapleLeaf dominate the world.")
                .header("appchannel", self.app_channel.clone())
                .header("platform", "1")
                .header("cacherefresh", "1")
                .header("appversion", self.app_version.clone())
                .header("accesstoken", uuid_hex())
                .multipart(form);
            let response = call(self.http.as_ref(), request).await?;
            let parsed: UnicomEnvelope = parse_json(&response)?;
            if parsed.rsp.data.is_empty() {
                return Err(err_no_data(NAME));
            }
            Ok(parsed.rsp.data)
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
        (entry().factory)(&ProviderItem::new(NAME), mock).expect("构造 chinaunicom 失败")
    }

    #[tokio::test]
    async fn uploads_with_app_headers() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"RSP":{"DATA":"https://unicom.example/a.png"}}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://unicom.example/a.png");
        let request = mock.last();
        assert_eq!(request.url, ENDPOINT);
        assert_eq!(
            request.header_value("clientsign"),
            Some("MapleLeaf dominate the world.")
        );
        assert_eq!(request.header_value("appchannel"), Some("oppo_woapp"));
        assert_eq!(request.header_value("appversion"), Some("7.8.2"));
        assert_eq!(request.header_value("platform"), Some("1"));
        assert_eq!(request.header_value("accesstoken").unwrap().len(), 32);
        let form = request.multipart_form().expect("应当是 multipart");
        assert_eq!(form.file_part("file").unwrap().data, b"bytes");
    }

    #[tokio::test]
    async fn custom_version_and_channel_are_sent() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"RSP":{"DATA":"https://u/a.png"}}"#,
        )]);
        let provider = (entry().factory)(
            &ProviderItem::new(NAME)
                .with_config("appVersion", "9.9.9")
                .with_config("appChannel", "huawei"),
            mock.clone(),
        )
        .unwrap();
        provider
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        let request = mock.last();
        assert_eq!(request.header_value("appversion"), Some("9.9.9"));
        assert_eq!(request.header_value("appchannel"), Some("huawei"));
    }

    #[tokio::test]
    async fn missing_data_is_an_error() {
        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"RSP":{}}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "chinaunicom: 响应缺少 DATA");
    }
}
