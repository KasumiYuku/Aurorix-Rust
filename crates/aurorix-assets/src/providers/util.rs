use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::error::AssetError;
use crate::http::HttpResponse;

pub fn cfg_str(config: &Map<String, Value>, key: &str, default: &str) -> String {
    match config.get(key) {
        Some(Value::String(value)) if !value.is_empty() => value.clone(),
        _ => default.to_string(),
    }
}

pub fn cfg_required(config: &Map<String, Value>, key: &str) -> Option<String> {
    match config.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

pub fn cfg_int(config: &Map<String, Value>, key: &str, default: i64) -> i64 {
    match config.get(key) {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(default),
        Some(Value::String(text)) => text.parse::<i64>().unwrap_or(default),
        _ => default,
    }
}

pub fn skey_from_cookie(cookie: &str) -> String {
    for segment in cookie.split(';') {
        if let Some((key, value)) = segment.trim().split_once('=') {
            if key == "skey" {
                return value.to_string();
            }
        }
    }
    String::new()
}

pub fn mime_ext(mime: &str) -> String {
    match mime.split_once('/') {
        Some((_, tail)) if !tail.is_empty() => tail.to_string(),
        _ => "jpg".to_string(),
    }
}

pub fn mime_or(mime: &str) -> String {
    if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime.to_string()
    }
}

pub fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub fn err_no_data(provider: &str) -> AssetError {
    AssetError::message(format!("{provider}: 响应缺少 DATA"))
}

pub fn wrap(context: &str, err: AssetError) -> AssetError {
    AssetError::message(format!("{context}: {err}"))
}

pub fn parse_body<T: DeserializeOwned>(response: &HttpResponse) -> Result<T, AssetError> {
    crate::http::parse_json(response)
}

pub fn regex_origin_url(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let Some(start) = text.find("\"origin_url\":\"") else {
        return String::new();
    };
    let rest = &text[start + "\"origin_url\":\"".len()..];
    let Some(end) = rest.find('"') else {
        return String::new();
    };
    rest[..end].replace("\\/", "/")
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

pub fn merge_query(url: &str, form: &Map<String, Value>) -> String {
    if form.is_empty() {
        return url.to_string();
    }
    let (base, existing) = match url.split_once('?') {
        Some((base, query)) => (base, query),
        None => (url, ""),
    };
    let mut pairs: Vec<(String, String)> = Vec::new();
    for segment in existing.split('&').filter(|item| !item.is_empty()) {
        let (key, value) = match segment.split_once('=') {
            Some((key, value)) => (key, value),
            None => (segment, ""),
        };
        pairs.push((decode_component(key), decode_component(value)));
    }
    for (key, value) in form {
        let value = match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        pairs.retain(|(existing, _)| existing != key);
        pairs.push((key.clone(), value));
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let query = pairs
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                crate::sign::urlencode_go_compat(key),
                crate::sign::urlencode_go_compat(value)
            )
        })
        .collect::<Vec<String>>()
        .join("&");
    format!("{base}?{query}")
}

fn decode_component(value: &str) -> String {
    percent_encoding::percent_decode_str(&value.replace('+', " "))
        .decode_utf8_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> Map<String, Value> {
        let mut config = Map::new();
        config.insert("text".to_string(), json!("value"));
        config.insert("empty".to_string(), json!(""));
        config.insert("num".to_string(), json!(7));
        config.insert("strnum".to_string(), json!("9"));
        config
    }

    #[test]
    fn config_helpers_follow_go_semantics() {
        let config = config();
        assert_eq!(cfg_str(&config, "text", "d"), "value");
        assert_eq!(cfg_str(&config, "empty", "d"), "d");
        assert_eq!(cfg_str(&config, "missing", "d"), "d");
        assert_eq!(cfg_required(&config, "text"), Some("value".to_string()));
        assert_eq!(cfg_required(&config, "empty"), None);
        assert_eq!(cfg_int(&config, "num", 1), 7);
        assert_eq!(cfg_int(&config, "strnum", 1), 9);
        assert_eq!(cfg_int(&config, "missing", 30), 30);
    }

    #[test]
    fn skey_is_extracted_from_cookie() {
        assert_eq!(skey_from_cookie("a=1; skey=@abc123; b=2"), "@abc123");
        assert_eq!(skey_from_cookie("noskey=1"), "");
        assert_eq!(skey_from_cookie(""), "");
    }

    #[test]
    fn mime_helpers_match_go() {
        assert_eq!(mime_ext("image/png"), "png");
        assert_eq!(mime_ext("image/svg+xml"), "svg+xml");
        assert_eq!(mime_ext(""), "jpg");
        assert_eq!(mime_or(""), "application/octet-stream");
        assert_eq!(mime_or("image/png"), "image/png");
    }

    #[test]
    fn origin_url_is_extracted_and_unescaped() {
        let raw = br#"callback({"origin_url":"http:\/\/a.b\/c.jpg","x":1})"#;
        assert_eq!(regex_origin_url(raw), "http://a.b/c.jpg");
        assert_eq!(regex_origin_url(b"nothing"), "");
        assert_eq!(regex_origin_url(br#""origin_url":"unterminated"#), "");
    }

    #[test]
    fn truncate_never_splits_characters() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("abcdef", 3), "abc");
        assert_eq!(truncate("图片图片", 6), "图片");
        assert_eq!(truncate("图片图片", 4), "图");
    }

    #[test]
    fn merge_query_sorts_and_encodes() {
        let mut form = Map::new();
        form.insert("b".to_string(), json!("2"));
        form.insert("a b".to_string(), json!("x y"));
        assert_eq!(
            merge_query("https://cnb.cool/upload", &form),
            "https://cnb.cool/upload?a%20b=x%20y&b=2"
        );
        assert_eq!(
            merge_query("https://cnb.cool/upload?z=1&b=9", &form),
            "https://cnb.cool/upload?a%20b=x%20y&b=2&z=1"
        );
        assert_eq!(
            merge_query("https://cnb.cool/upload", &Map::new()),
            "https://cnb.cool/upload"
        );
    }
}
