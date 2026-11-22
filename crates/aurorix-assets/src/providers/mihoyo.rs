use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AssetError;
use crate::http::{call, parse_json, HttpClient, HttpMethod, HttpRequest, Multipart};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::{base64_decode, md5_hex};

use super::util::{cfg_required, mime_ext, mime_or};

pub const NAME: &str = "mihoyo";
const PARAMS_ENDPOINT: &str = "https://api-takumi.miyoushe.com/upload/outer/getParamsByAccount";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36";
const BASE_FIELDS: [&str; 10] = [
    "name",
    "key",
    "callback",
    "success_action_status",
    "x:extra",
    "x-oss-content-type",
    "OSSAccessKeyId",
    "policy",
    "signature",
    "x-oss-object-acl",
];

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "米游社",
        vec![ConfigField::password("cookie", "米游社登录 Cookie")
            .required()
            .description("含 stuid / stoken / ltoken，抓包网页端或 App 已登录请求的请求头 Cookie")],
        Box::new(|item, http| {
            let Some(cookie) = cfg_required(&item.config, "cookie") else {
                return Err(AssetError::message("mihoyo: cookie 必填"));
            };
            Ok(Arc::new(MihoyoProvider { http, cookie }))
        }),
    )
}

pub struct MihoyoProvider {
    http: Arc<dyn HttpClient>,
    cookie: String,
}

#[derive(Debug, Default, Deserialize)]
struct MihoyoOss {
    #[serde(default)]
    host: String,
    #[serde(default)]
    policy: String,
    #[serde(default)]
    callback: String,
    #[serde(default, rename = "accessid")]
    access_id: String,
    #[serde(default)]
    signature: String,
    #[serde(default, rename = "object_acl")]
    object_acl: String,
    #[serde(default, rename = "x_oss_content_type")]
    oss_content_type: String,
    #[serde(default)]
    callback_var: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct MihoyoParams {
    #[serde(default, rename = "file_name")]
    file_name: String,
    #[serde(default)]
    oss: Option<MihoyoOss>,
}

#[derive(Debug, Deserialize)]
struct MihoyoEnvelope {
    #[serde(default)]
    retcode: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<MihoyoParams>,
}

#[derive(Debug, Default, Deserialize)]
struct MihoyoResult {
    #[serde(default)]
    data: Option<MihoyoUploaded>,
}

#[derive(Debug, Default, Deserialize)]
struct MihoyoUploaded {
    #[serde(default)]
    url: String,
}

impl MihoyoProvider {
    async fn params(&self, md5: &str, ext: &str) -> Result<MihoyoParams, AssetError> {
        let body = json!({
            "md5": md5,
            "ext": ext,
            "biz": "community",
            "extra": "{\"upload_source\":\"villa_chat\"}",
            "support_content_type": true,
            "support_extra_form_data": true,
        });
        let request = HttpRequest::new(HttpMethod::Post, PARAMS_ENDPOINT)
            .header("cookie", self.cookie.clone())
            .header("Origin", "https://dby.miyoushe.com")
            .header("Referer", "https://dby.miyoushe.com/")
            .header("User-Agent", USER_AGENT)
            .json(&body)?;
        let response = call(self.http.as_ref(), request).await?;
        let envelope: MihoyoEnvelope = parse_json(&response)?;
        if envelope.retcode != 0 || envelope.data.is_none() {
            if envelope.retcode == -100 || envelope.message.contains("登录") {
                return Err(AssetError::message(format!(
                    "mihoyo: Cookie 已失效/过期 (retcode={} {})，请更新后重试",
                    envelope.retcode, envelope.message
                )));
            }
            return Err(AssetError::message(format!(
                "mihoyo: 获取上传参数失败 (retcode={} {})",
                envelope.retcode, envelope.message
            )));
        }
        Ok(envelope.data.unwrap_or_default())
    }
}

impl Provider for MihoyoProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let ext = mime_ext(&input.mime);
            let params = self.params(&md5_hex(input.bytes), &ext).await?;
            let Some(oss) = params.oss else {
                return Err(AssetError::message(
                    "mihoyo: 获取上传参数失败，缺少 OSS 信息",
                ));
            };
            if oss.host.is_empty() || params.file_name.is_empty() {
                return Err(AssetError::message(
                    "mihoyo: 获取上传参数失败，缺少 OSS 信息",
                ));
            }
            let base_name = crate::http::file_base_name(&params.file_name);
            let mut form = Multipart::new()
                .field("name", base_name.clone())
                .field("key", params.file_name.clone())
                .field("callback", oss.callback.clone())
                .field("success_action_status", "200")
                .field(
                    "x:extra",
                    oss.callback_var.get("x:extra").cloned().unwrap_or_default(),
                )
                .field("x-oss-content-type", oss.oss_content_type.clone())
                .field("OSSAccessKeyId", oss.access_id.clone())
                .field("policy", oss.policy.clone())
                .field("signature", oss.signature.clone());
            let acl = if oss.object_acl.is_empty() {
                "default".to_string()
            } else {
                oss.object_acl.clone()
            };
            form = form.field("x-oss-object-acl", acl);
            for (key, value) in eq_fields(&oss.policy) {
                if !BASE_FIELDS.contains(&key.as_str()) {
                    form = form.field(key, value);
                }
            }
            form = form.file(
                "file",
                base_name,
                mime_or(&input.mime),
                input.bytes.to_vec(),
            );
            let request = HttpRequest::new(HttpMethod::Post, oss.host.clone())
                .header("Referer", "https://dby.miyoushe.com/")
                .multipart(form);
            let response = call(self.http.as_ref(), request).await?;
            let result: MihoyoResult = parse_json(&response)?;
            match result.data {
                Some(uploaded) if !uploaded.url.is_empty() => Ok(uploaded.url),
                _ => Err(AssetError::message("mihoyo: 上传失败，未返回 URL")),
            }
        })
    }
}

fn eq_fields(policy: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let Some(decoded) = base64_decode(policy) else {
        return fields;
    };
    let Ok(parsed) = serde_json::from_slice::<Value>(&decoded) else {
        return fields;
    };
    let Some(conditions) = parsed.get("conditions").and_then(Value::as_array) else {
        return fields;
    };
    for condition in conditions {
        let Some(items) = condition.as_array() else {
            continue;
        };
        if items.len() < 3 {
            continue;
        }
        let op = items[0].as_str().unwrap_or_default();
        let Some(field) = items[1].as_str() else {
            continue;
        };
        if op != "eq" || !field.starts_with('$') {
            continue;
        }
        let value = match &items[2] {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        fields.insert(field.trim_start_matches('$').to_string(), value);
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderItem;
    use crate::http::HttpResponse;
    use crate::testkit::MockHttp;
    use crate::testkit::ResultExt as _;

    fn policy() -> String {
        let raw = json!({
            "conditions": [
                ["eq", "$key", "tos-cn/x.png"],
                ["eq", "$name", "x.png"],
                ["eq", "$x-oss-content-type", "image/png"],
                ["eq", "$x:extra", "extra-value"],
                ["starts-with", "$Content-Type", "image/"]
            ]
        });
        crate::sign::base64_encode(raw.to_string().as_bytes())
    }

    fn provider(mock: Arc<MockHttp>) -> Arc<dyn Provider> {
        (entry().factory)(
            &ProviderItem::new(NAME).with_config("cookie", "stuid=1; stoken=2"),
            mock,
        )
        .expect("构造 mihoyo 失败")
    }

    #[tokio::test]
    async fn two_step_upload_uses_server_side_policy() {
        let params = json!({
            "retcode": 0,
            "data": {
                "file_name": "upload/2024/x.png",
                "oss": {
                    "host": "https://oss.mihoyo.com",
                    "policy": policy(),
                    "callback": "cb",
                    "accessid": "AK",
                    "signature": "SIG",
                    "object_acl": "",
                    "x_oss_content_type": "image/png",
                    "callback_var": {"x:extra": "extra-value"}
                }
            }
        });
        let mock = MockHttp::shared(vec![
            HttpResponse::json(params.to_string()),
            HttpResponse::json(r#"{"data":{"url":"https://upload.mihoyo.com/x.png"}}"#),
        ]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"png", "x.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://upload.mihoyo.com/x.png");

        let prepare = mock.request(0);
        assert_eq!(prepare.url, PARAMS_ENDPOINT);
        assert_eq!(prepare.header_value("cookie"), Some("stuid=1; stoken=2"));
        assert_eq!(
            prepare.header_value("Origin"),
            Some("https://dby.miyoushe.com")
        );
        let body: Value = serde_json::from_slice(prepare.body_bytes().unwrap()).unwrap();
        assert_eq!(body["md5"], md5_hex(b"png"));
        assert_eq!(body["ext"], "png");
        assert_eq!(body["biz"], "community");
        assert_eq!(body["extra"], "{\"upload_source\":\"villa_chat\"}");
        assert_eq!(body["support_content_type"], true);

        let upload = mock.request(1);
        assert_eq!(upload.url, "https://oss.mihoyo.com");
        assert_eq!(
            upload.header_value("Referer"),
            Some("https://dby.miyoushe.com/")
        );
        let form = upload.multipart_form().expect("应当是 multipart");
        assert_eq!(form.field_value("name"), Some("x.png"));
        assert_eq!(form.field_value("key"), Some("upload/2024/x.png"));
        assert_eq!(form.field_value("x-oss-object-acl"), Some("default"));
        assert_eq!(form.field_value("x:extra"), Some("extra-value"));
        assert_eq!(form.field_value("x-oss-content-type"), Some("image/png"));
        assert!(form.field_value("Content-Type").is_none());
        assert_eq!(form.file_part("file").unwrap().filename, "x.png");
    }

    #[tokio::test]
    async fn expired_cookie_is_reported() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"retcode":-100,"message":"请先登录"}"#,
        )]);
        let err = provider(mock)
            .upload(UploadInput::new(b"png", "x.png", "image/png"))
            .await
            .failure();
        assert!(err.to_string().contains("mihoyo: Cookie 已失效/过期"));
    }

    #[tokio::test]
    async fn missing_data_is_an_error() {
        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"retcode":0}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"png", "x.png", "image/png"))
            .await
            .failure();
        assert!(err
            .to_string()
            .contains("mihoyo: 获取上传参数失败 (retcode=0"));
    }

    #[test]
    fn policy_conditions_are_flattened() {
        let fields = eq_fields(&policy());
        assert_eq!(fields.get("key").map(String::as_str), Some("tos-cn/x.png"));
        assert_eq!(
            fields.get("x:extra").map(String::as_str),
            Some("extra-value")
        );
        assert!(!fields.contains_key("Content-Type"));
        assert!(eq_fields("not-base64!!").is_empty());
    }

    #[test]
    fn cookie_is_required() {
        let err = (entry().factory)(&ProviderItem::new(NAME), MockHttp::shared(vec![])).failure();
        assert_eq!(err.to_string(), "mihoyo: cookie 必填");
    }
}
