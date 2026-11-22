use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use crate::error::AssetError;
use crate::http::{call, HttpClient, HttpMethod, HttpRequest};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::{base64_encode, bkn};

use super::util::{cfg_required, skey_from_cookie, truncate};

pub const NAME: &str = "homework";
const ENDPOINT: &str = "https://qun.qq.com/cgi-bin/hw/util/image";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 6.2; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) QQ/9.7.1.28934 Chrome/43.0.2357.134 Safari/537.36 QBCore/3.43.1298.400 QQBrowser/9.0.2524.400";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "QQ 群作业",
        vec![ConfigField::password("cookie", "qun.qq.com Cookie（含 skey）").required()],
        Box::new(|item, http| {
            let Some(cookie) = cfg_required(&item.config, "cookie") else {
                return Err(AssetError::message("homework: cookie 必填"));
            };
            let skey = skey_from_cookie(&cookie);
            Ok(Arc::new(HomeworkProvider { http, cookie, skey }))
        }),
    )
}

pub struct HomeworkProvider {
    http: Arc<dyn HttpClient>,
    cookie: String,
    skey: String,
}

#[derive(Debug, Default, Deserialize)]
struct HomeworkEnvelope {
    #[serde(default)]
    cgicode: i64,
    #[serde(default)]
    retcode: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Value,
}

#[derive(Debug, Default, Deserialize)]
struct HomeworkData {
    #[serde(default)]
    url: Option<HomeworkUrl>,
}

#[derive(Debug, Default, Deserialize)]
struct HomeworkUrl {
    #[serde(default)]
    origin: String,
}

impl Provider for HomeworkProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let form = vec![
                ("pic".to_string(), base64_encode(input.bytes)),
                ("client_type".to_string(), "1".to_string()),
                ("bkn".to_string(), bkn(&self.skey)),
            ];
            let request = HttpRequest::new(HttpMethod::Post, ENDPOINT)
                .header("Cookie", self.cookie.clone())
                .header("Content-Type", "application/x-www-form-urlencoded")
                .header("Accept", "*/*")
                .header("Origin", "https://qun.qq.com")
                .header(
                    "Referer",
                    "https://qun.qq.com/homework/p/features/index.html",
                )
                .header("User-Agent", USER_AGENT)
                .form(form);
            let response = call(self.http.as_ref(), request).await?;
            let raw = response.text();
            let envelope: HomeworkEnvelope =
                serde_json::from_slice(&response.body).map_err(|_| {
                    AssetError::message(format!("homework: 响应解析失败: {}", truncate(&raw, 200)))
                })?;
            if envelope.cgicode != 0 || envelope.retcode != 0 {
                return Err(AssetError::message(format!(
                    "homework: 上传失败({}): {}",
                    envelope.cgicode, envelope.msg
                )));
            }
            let data: HomeworkData = match &envelope.data {
                Value::Object(_) => {
                    serde_json::from_value(envelope.data.clone()).map_err(|_| {
                        AssetError::message(format!(
                            "homework: 响应解析失败: {}",
                            truncate(&raw, 200)
                        ))
                    })?
                }
                _ => HomeworkData::default(),
            };
            let origin = data.url.map(|url| url.origin).unwrap_or_default();
            if origin.is_empty() {
                return Err(AssetError::message(format!(
                    "homework: 响应缺少图片 URL: {}",
                    truncate(&raw, 200)
                )));
            }
            Ok(origin.replacen("p.qpic.cn", "p.qlogo.cn", 1))
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
            &ProviderItem::new(NAME).with_config("cookie", "skey=@hw; uin=o1"),
            mock,
        )
        .expect("构造 homework 失败")
    }

    #[tokio::test]
    async fn posts_form_and_rewrites_qpic_host() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"cgicode":0,"retcode":0,"msg":"","data":{"url":{"origin":"https://p.qpic.cn/hw/a.png"}}}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://p.qlogo.cn/hw/a.png");
        let request = mock.last();
        assert_eq!(request.url, ENDPOINT);
        assert_eq!(
            request.header_value("Content-Type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(request.header_value("Origin"), Some("https://qun.qq.com"));
        assert_eq!(
            request.header_value("Referer"),
            Some("https://qun.qq.com/homework/p/features/index.html")
        );
        assert_eq!(request.form_value("pic"), Some("Ynl0ZXM="));
        assert_eq!(request.form_value("client_type"), Some("1"));
        assert_eq!(request.form_value("bkn"), Some(bkn("@hw").as_str()));
    }

    #[tokio::test]
    async fn error_codes_and_empty_data_are_reported() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"cgicode":100,"retcode":0,"msg":"没有权限"}"#,
        )]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "homework: 上传失败(100): 没有权限");

        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"cgicode":0,"retcode":0,"data":{"url":{}}}"#,
        )]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert!(err.to_string().starts_with("homework: 响应缺少图片 URL: "));
    }

    #[tokio::test]
    async fn non_object_data_yields_missing_url() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"cgicode":0,"retcode":0,"data":""}"#,
        )]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert!(err.to_string().starts_with("homework: 响应缺少图片 URL: "));
    }

    #[test]
    fn cookie_is_required() {
        let err = (entry().factory)(&ProviderItem::new(NAME), MockHttp::shared(vec![])).failure();
        assert_eq!(err.to_string(), "homework: cookie 必填");
    }
}
