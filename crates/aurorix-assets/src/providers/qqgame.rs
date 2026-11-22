use std::sync::Arc;

use serde::Deserialize;

use crate::error::AssetError;
use crate::http::{call, parse_json, HttpClient, HttpMethod, HttpRequest};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::{cos_sign, uuid_hex};

use super::util::{cfg_str, mime_ext, now_millis, now_unix};

pub const NAME: &str = "qqgame";
const AUTH_ENDPOINT: &str = "https://game.q.qq.com/cgi-bin/cosTmpAuth";
const COS_HOST: &str = "sopstatic-1251316161.cos.ap-nanjing.myqcloud.com";
const CDN_HOST: &str = "https://sopstatic.gtimg.cn/";
const HEADER_LIST: &str = "content-length;host;x-cos-storage-class";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "QQ 游戏 COS",
        vec![ConfigField::text("opensopUid", "opensop_uid").default_value("A")],
        Box::new(|item, http| {
            Ok(Arc::new(QqgameProvider {
                http,
                uid: cfg_str(&item.config, "opensopUid", "A"),
            }))
        }),
    )
}

pub struct QqgameProvider {
    http: Arc<dyn HttpClient>,
    uid: String,
}

#[derive(Debug, Default, Deserialize)]
struct QqgameAuth {
    #[serde(default)]
    credentials: Option<QqgameCredentials>,
}

#[derive(Debug, Default, Deserialize)]
struct QqgameCredentials {
    #[serde(default, rename = "sessionToken")]
    session_token: String,
    #[serde(default, rename = "tmpSecretId")]
    tmp_secret_id: String,
    #[serde(default, rename = "tmpSecretKey")]
    tmp_secret_key: String,
}

impl Provider for QqgameProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let request = HttpRequest::new(HttpMethod::Get, AUTH_ENDPOINT)
                .header("Host", "game.q.qq.com")
                .header("Cookie", format!("opensop_uid={}", self.uid))
                .header(
                    "traceparent",
                    format!("00-{}-{}-01", uuid_hex(), &uuid_hex()[..16]),
                )
                .header(
                    "Referer",
                    "https://game.q.qq.com/setting/game-management?type=create",
                )
                .header("User-Agent", USER_AGENT);
            let response = call(self.http.as_ref(), request).await?;
            let auth: QqgameAuth = parse_json(&response)?;
            let Some(credentials) = auth.credentials else {
                return Err(AssetError::message("qqgame: 获取临时凭证失败"));
            };
            let ext = mime_ext(&input.mime);
            let now = now_unix();
            let cos_path = format!(
                "tqgame/{}/{}{}.{ext}",
                self.uid,
                now,
                now_millis() % 1_000_000
            );
            let expire = now + 300;
            let content_length = input.bytes.len().to_string();
            let signed = vec![
                ("content-length".to_string(), content_length.clone()),
                ("host".to_string(), COS_HOST.to_string()),
                ("x-cos-storage-class".to_string(), "STANDARD".to_string()),
            ];
            let (signature, q_key_time) = cos_sign(
                "put",
                &format!("/{cos_path}"),
                &signed,
                &credentials.tmp_secret_key,
                now,
                expire,
            );
            let authorization = format!(
                "q-sign-algorithm=sha1&q-ak={}&q-sign-time={q_key_time}&q-key-time={q_key_time}&q-header-list={HEADER_LIST}&q-url-param-list=&q-signature={signature}",
                credentials.tmp_secret_id
            );
            let request =
                HttpRequest::new(HttpMethod::Put, format!("https://{COS_HOST}/{cos_path}"))
                    .header("content-length", content_length)
                    .header("host", COS_HOST)
                    .header("x-cos-storage-class", "STANDARD")
                    .header("Authorization", authorization)
                    .header("x-cos-security-token", credentials.session_token)
                    .header("User-Agent", "Mozilla/5.0")
                    .header("Referer", "https://game.q.qq.com/")
                    .bytes(input.bytes.to_vec());
            call(self.http.as_ref(), request).await?;
            Ok(format!("{CDN_HOST}{cos_path}"))
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
        (entry().factory)(
            &ProviderItem::new(NAME).with_config("opensopUid", "U9"),
            mock,
        )
        .expect("构造 qqgame 失败")
    }

    #[tokio::test]
    async fn fetches_temp_credentials_then_puts_to_cos() {
        let mock = MockHttp::shared(vec![
            HttpResponse::json(
                r#"{"credentials":{"sessionToken":"ST","tmpSecretId":"ID","tmpSecretKey":"KEY"}}"#,
            ),
            HttpResponse::ok(),
        ]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        assert!(url.starts_with("https://sopstatic.gtimg.cn/tqgame/U9/"));
        assert!(url.ends_with(".png"));

        let auth = mock.request(0);
        assert_eq!(auth.method, HttpMethod::Get);
        assert_eq!(auth.url, AUTH_ENDPOINT);
        assert_eq!(auth.header_value("Host"), Some("game.q.qq.com"));
        assert_eq!(auth.header_value("Cookie"), Some("opensop_uid=U9"));
        let traceparent = auth.header_value("traceparent").unwrap();
        let parts: Vec<&str> = traceparent.split('-').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "00");
        assert_eq!(parts[1].len(), 32);
        assert_eq!(parts[2].len(), 16);
        assert_eq!(parts[3], "01");

        let put = mock.request(1);
        assert_eq!(put.method, HttpMethod::Put);
        assert!(put
            .url
            .starts_with("https://sopstatic-1251316161.cos.ap-nanjing.myqcloud.com/tqgame/U9/"));
        assert_eq!(put.header_value("x-cos-security-token"), Some("ST"));
        assert_eq!(put.header_value("x-cos-storage-class"), Some("STANDARD"));
        let authorization = put.header_value("Authorization").unwrap();
        assert!(authorization.starts_with("q-sign-algorithm=sha1&q-ak=ID&q-sign-time="));
        assert!(authorization.contains("q-header-list=content-length;host;x-cos-storage-class"));
        assert_eq!(put.body_bytes(), Some(&b"bytes"[..]));
    }

    #[tokio::test]
    async fn missing_credentials_is_an_error() {
        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"credentials":null}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "qqgame: 获取临时凭证失败");
    }

    #[test]
    fn default_uid_is_letter_a() {
        let mock = MockHttp::shared(vec![
            HttpResponse::json(
                r#"{"credentials":{"sessionToken":"ST","tmpSecretId":"ID","tmpSecretKey":"KEY"}}"#,
            ),
            HttpResponse::ok(),
        ]);
        let provider = (entry().factory)(&ProviderItem::new(NAME), mock).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let url = rt
            .block_on(provider.upload(UploadInput::new(b"b", "a.png", "image/png")))
            .unwrap();
        assert!(url.contains("/tqgame/A/"));
    }
}
