use std::future::Future;
use std::pin::Pin;

use serde::de::DeserializeOwned;

use crate::error::AssetError;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub const MAX_REDIRECTS: usize = 5;
pub const FETCH_TIMEOUT_SECS: u64 = 15;
pub const HEAD_TIMEOUT_SECS: u64 = 8;
pub const MAX_FETCH_BYTES: usize = 20 << 20;
pub const ERROR_BODY_LIMIT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Head,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultipartEntry {
    Field {
        name: String,
        value: String,
    },
    File {
        name: String,
        filename: String,
        mime: String,
        data: Vec<u8>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Multipart {
    entries: Vec<MultipartEntry>,
}

impl Multipart {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn field(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.entries.push(MultipartEntry::Field {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    pub fn file(
        mut self,
        name: impl Into<String>,
        filename: impl Into<String>,
        mime: impl Into<String>,
        data: impl Into<Vec<u8>>,
    ) -> Self {
        self.entries.push(MultipartEntry::File {
            name: name.into(),
            filename: file_base_name(&filename.into()),
            mime: mime.into(),
            data: data.into(),
        });
        self
    }

    pub fn entries(&self) -> &[MultipartEntry] {
        &self.entries
    }

    pub fn field_value(&self, name: &str) -> Option<&str> {
        self.entries.iter().find_map(|entry| match entry {
            MultipartEntry::Field { name: key, value } if key == name => Some(value.as_str()),
            _ => None,
        })
    }

    pub fn file_part(&self, name: &str) -> Option<FilePart<'_>> {
        self.entries.iter().find_map(|entry| match entry {
            MultipartEntry::File {
                name: key,
                filename,
                mime,
                data,
            } if key == name => Some(FilePart {
                filename,
                mime,
                data,
            }),
            _ => None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FilePart<'a> {
    pub filename: &'a str,
    pub mime: &'a str,
    pub data: &'a [u8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpBody {
    Empty,
    Bytes(Vec<u8>),
    Form(Vec<(String, String)>),
    Multipart(Multipart),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: HttpBody,
}

impl HttpRequest {
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: HttpBody::Empty,
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_headers(mut self, headers: &[(&str, &str)]) -> Self {
        for (name, value) in headers {
            self.headers
                .push(((*name).to_string(), (*value).to_string()));
        }
        self
    }

    pub fn bytes(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.body = HttpBody::Bytes(data.into());
        self
    }

    pub fn form(mut self, pairs: Vec<(String, String)>) -> Self {
        self.body = HttpBody::Form(pairs);
        self
    }

    pub fn multipart(mut self, form: Multipart) -> Self {
        self.body = HttpBody::Multipart(form);
        self
    }

    pub fn json(mut self, value: &serde_json::Value) -> Result<Self, AssetError> {
        let raw = serde_json::to_vec(value)
            .map_err(|err| AssetError::Json(format!("请求体序列化失败: {err}")))?;
        self.body = HttpBody::Bytes(raw);
        self.headers
            .push(("Content-Type".to_string(), "application/json".to_string()));
        Ok(self)
    }

    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn body_bytes(&self) -> Option<&[u8]> {
        match &self.body {
            HttpBody::Bytes(data) => Some(data),
            _ => None,
        }
    }

    pub fn form_pairs(&self) -> Option<&[(String, String)]> {
        match &self.body {
            HttpBody::Form(pairs) => Some(pairs),
            _ => None,
        }
    }

    pub fn form_value(&self, name: &str) -> Option<&str> {
        self.form_pairs()?
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn multipart_form(&self) -> Option<&Multipart> {
        match &self.body {
            HttpBody::Multipart(form) => Some(form),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub final_url: String,
}

impl Default for HttpResponse {
    fn default() -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
            final_url: String::new(),
        }
    }
}

impl HttpResponse {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }

    pub fn ok() -> Self {
        Self::default()
    }

    pub fn with_status(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
            ..Self::default()
        }
    }

    pub fn json(body: impl Into<String>) -> Self {
        Self {
            body: body.into().into_bytes(),
            ..Self::default()
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_final_url(mut self, url: impl Into<String>) -> Self {
        self.final_url = url.into();
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

pub trait HttpClient: Send + Sync {
    fn execute<'a>(
        &'a self,
        request: HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, AssetError>>;
}

pub struct ReqwestClient {
    client: reqwest::Client,
}

impl Default for ReqwestClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ReqwestClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub fn inner(&self) -> &reqwest::Client {
        &self.client
    }
}

fn to_reqwest_method(method: HttpMethod) -> reqwest::Method {
    match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Head => reqwest::Method::HEAD,
    }
}

fn to_reqwest_form(form: &Multipart) -> reqwest::multipart::Form {
    let mut out = reqwest::multipart::Form::new();
    for entry in form.entries() {
        match entry {
            MultipartEntry::Field { name, value } => {
                out = out.text(name.clone(), value.clone());
            }
            MultipartEntry::File {
                name,
                filename,
                mime,
                data,
            } => {
                let plain =
                    || reqwest::multipart::Part::bytes(data.clone()).file_name(filename.clone());
                let part = if mime.is_empty() {
                    plain()
                } else {
                    plain().mime_str(mime).unwrap_or_else(|_| plain())
                };
                out = out.part(name.clone(), part);
            }
        }
    }
    out
}

impl HttpClient for ReqwestClient {
    fn execute<'a>(
        &'a self,
        request: HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, AssetError>> {
        Box::pin(async move {
            let method = to_reqwest_method(request.method);
            let mut builder = self.client.request(method, &request.url);
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            builder = match request.body {
                HttpBody::Empty => builder,
                HttpBody::Bytes(data) => builder.body(data),
                HttpBody::Form(pairs) => builder.form(&pairs),
                HttpBody::Multipart(form) => builder.multipart(to_reqwest_form(&form)),
            };
            let response = builder
                .send()
                .await
                .map_err(|err| AssetError::Transport(err.to_string()))?;
            let status = response.status().as_u16();
            let final_url = response.url().to_string();
            let headers = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        value.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect();
            let body = if request.method == HttpMethod::Head {
                Vec::new()
            } else {
                response
                    .bytes()
                    .await
                    .map_err(|err| AssetError::Transport(err.to_string()))?
                    .to_vec()
            };
            Ok(HttpResponse {
                status,
                headers,
                body,
                final_url,
            })
        })
    }
}

pub async fn call(http: &dyn HttpClient, request: HttpRequest) -> Result<HttpResponse, AssetError> {
    let response = http.execute(request).await?;
    if (200..300).contains(&response.status) {
        return Ok(response);
    }
    Err(AssetError::status(
        response.status,
        truncate_for_log(&response.body),
    ))
}

pub fn parse_json<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, AssetError> {
    serde_json::from_slice(&response.body).map_err(|err| AssetError::Json(err.to_string()))
}

pub fn truncate_for_log(raw: &[u8]) -> String {
    let slice = &raw[..raw.len().min(ERROR_BODY_LIMIT)];
    String::from_utf8_lossy(slice).to_string()
}

pub fn file_base_name(name: &str) -> String {
    match name.rsplit_once('/') {
        Some((_, tail)) if !tail.is_empty() => tail.to_string(),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::MockHttp;
    use std::sync::Arc;

    #[test]
    fn file_base_name_strips_directories() {
        assert_eq!(file_base_name("a/b/c.png"), "c.png");
        assert_eq!(file_base_name("c.png"), "c.png");
        assert_eq!(file_base_name(""), "");
        assert_eq!(file_base_name("a/"), "a/");
    }

    #[test]
    fn multipart_looks_up_fields_and_files() {
        let form = Multipart::new().field("bucket", "album").file(
            "file",
            "/tmp/x/photo.png",
            "image/png",
            vec![1u8, 2, 3],
        );
        assert_eq!(form.field_value("bucket"), Some("album"));
        assert_eq!(form.field_value("missing"), None);
        let part = form.file_part("file").expect("文件字段应当存在");
        assert_eq!(part.filename, "photo.png");
        assert_eq!(part.mime, "image/png");
        assert_eq!(part.data, &[1, 2, 3]);
        assert!(form.file_part("bucket").is_none());
    }

    #[test]
    fn request_builders_record_headers_and_body() {
        let request = HttpRequest::new(HttpMethod::Post, "https://example.com/a b")
            .header("X-Test", "1")
            .with_headers(&[("accept", "application/json")])
            .bytes(vec![9u8]);
        assert_eq!(request.method.as_str(), "POST");
        assert_eq!(request.header_value("x-test"), Some("1"));
        assert_eq!(request.header_value("Accept"), Some("application/json"));
        assert_eq!(request.body_bytes(), Some(&[9u8][..]));
        assert!(request.form_pairs().is_none());
    }

    #[test]
    fn json_body_sets_content_type() {
        let request = HttpRequest::new(HttpMethod::Post, "https://example.com")
            .json(&serde_json::json!({"a": 1}))
            .expect("JSON 序列化应成功");
        assert_eq!(
            request.header_value("content-type"),
            Some("application/json")
        );
        assert_eq!(request.body_bytes(), Some(br#"{"a":1}"#.as_slice()));
    }

    #[test]
    fn form_request_exposes_pairs() {
        let request = HttpRequest::new(HttpMethod::Post, "https://example.com").form(vec![
            ("b".to_string(), "2".to_string()),
            ("a".to_string(), "1".to_string()),
        ]);
        assert_eq!(request.form_value("a"), Some("1"));
        assert_eq!(request.form_value("b"), Some("2"));
        assert_eq!(request.form_value("c"), None);
    }

    #[tokio::test]
    async fn call_rejects_non_2xx_with_truncated_body() {
        let long = "x".repeat(500);
        let mock = Arc::new(MockHttp::new(vec![HttpResponse::with_status(502, long)]));
        let err = call(
            mock.as_ref(),
            HttpRequest::new(HttpMethod::Get, "https://e.com"),
        )
        .await
        .unwrap_err();
        match err {
            AssetError::Status { status, body } => {
                assert_eq!(status, 502);
                assert_eq!(body.len(), ERROR_BODY_LIMIT);
            }
            other => panic!("应报状态错误: {other}"),
        }
    }

    #[tokio::test]
    async fn call_accepts_2xx() {
        let mock = Arc::new(MockHttp::new(vec![
            HttpResponse::json("{}").with_header("Content-Type", "application/json")
        ]));
        let response = call(
            mock.as_ref(),
            HttpRequest::new(HttpMethod::Get, "https://e.com"),
        )
        .await
        .expect("2xx 应当成功");
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(response.text(), "{}");
    }
}
