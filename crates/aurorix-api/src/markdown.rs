use std::future::Future;
use std::pin::Pin;

use crate::error::ApiError;
use crate::image_box::ImageBox;
use crate::text::{truncate_runes, GUILD_TEXT_LIMIT};

pub use crate::text::strip_latex;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct StoredImage {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

pub trait ImageStore: Send + Sync {
    fn ensure(&self, url: &str) -> BoxFuture<'_, Result<String, ApiError>>;

    fn store(
        &self,
        _bytes: &[u8],
        _filename: &str,
    ) -> BoxFuture<'_, Result<StoredImage, ApiError>> {
        Box::pin(async move { Err(ApiError::Upload(String::from("当前图床不支持字节上传"))) })
    }

    fn publish(
        &self,
        _bytes: &[u8],
        _file_type: i32,
        _filename: &str,
    ) -> BoxFuture<'_, Result<String, ApiError>> {
        Box::pin(async move { Err(ApiError::Upload(String::from("当前图床不支持媒体转存"))) })
    }
}

pub const ZERO_WIDTH_SPACE: &str = "\u{200b}";

pub const CHANNEL_TEXT_LIMIT: usize = GUILD_TEXT_LIMIT;

const CMD_TAG_PREFIX: &str = "<qqbot-cmd-";


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedImage {
    pub alt: String,
    pub source: String,
    pub url: String,
    pub stored: bool,
    pub failure: Option<String>,
    pub filename: String,
    pub mime: String,
}

impl PreparedImage {
    pub fn byte_direct(&self) -> bool {
        !self.stored
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prepared {
    pub content: String,
    pub images: Vec<PreparedImage>,
    pub truncated: bool,
}

pub async fn prepare(markdown: &str, store: Option<&dyn ImageStore>, limit: usize) -> Prepared {
    prepare_with(markdown, store, limit, None).await
}

pub async fn prepare_with(
    markdown: &str,
    store: Option<&dyn ImageStore>,
    limit: usize,
    image_box: Option<(u32, u32)>,
) -> Prepared {
    if markdown.is_empty() {
        return Prepared::default();
    }
    let protected = protect_at(markdown);
    let (embedded, images) = match store {
        Some(store) => embed_images(&protected, store).await,
        None => (protected, Vec::new()),
    };
    let fitted = match image_box {
        Some((width, height)) => fit_images(&embedded, width, height),
        None => embedded,
    };
    let encoded = encode_cmd_tags(&fitted);
    let truncated = limit > 0 && encoded.chars().count() > limit;
    let content = if truncated {
        truncate_runes(&encoded, limit)
    } else {
        encoded
    };
    Prepared {
        content,
        images,
        truncated,
    }
}

pub fn protect_at(content: &str) -> String {
    if !content.contains('@') {
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len() + content.len() / 4);
    let mut previous: Option<char> = None;
    let mut chars = content.chars().peekable();
    while let Some(current) = chars.next() {
        let next = chars.peek().copied();
        let keep = match next {
            Some(following) => following.is_whitespace(),
            None => true,
        };
        let blocked = match previous {
            Some(previous) => is_word_char(previous) || previous == '<',
            None => false,
        };
        if current == '@' && !keep && !blocked {
            out.push('@');
            out.push_str(ZERO_WIDTH_SPACE);
        } else {
            out.push(current);
        }
        previous = Some(current);
    }
    out
}

fn is_word_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_' || value == '>'
}

pub fn encode_cmd_tags(content: &str) -> String {
    if !content.contains("qqbot-cmd-") {
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    while let Some(offset) = content[cursor..].find(CMD_TAG_PREFIX) {
        let start = cursor + offset;
        let Some(close) = content[start..].find('>') else {
            break;
        };
        let end = start + close + 1;
        out.push_str(&content[cursor..start]);
        let tag = &content[start..end];
        let name = tag_name(tag);
        if name == "input" || name == "enter" {
            out.push_str(&encode_tag(tag, &name));
        } else {
            out.push_str(tag);
        }
        cursor = end;
    }
    out.push_str(&content[cursor..]);
    out
}

fn tag_name(tag: &str) -> String {
    let rest = &tag[CMD_TAG_PREFIX.len()..];
    rest.chars()
        .take_while(|value| value.is_ascii_alphabetic())
        .collect()
}

fn encode_tag(tag: &str, name: &str) -> String {
    let encoded = rewrite_attrs(tag);
    if name != "input" || has_attr(&encoded, "show") {
        return encoded;
    }
    let text = attr_value(&encoded, "text").unwrap_or_default();
    inject_show(&encoded, &cmd_escape(&text))
}

fn rewrite_attrs(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len() + 8);
    let mut last = 0usize;
    let mut cursor = 0usize;
    while let Some((start, name_len)) = next_attr(tag, cursor) {
        let value_start = start + name_len + 2;
        let Some(close) = tag[value_start..].find('"') else {
            break;
        };
        let value_end = value_start + close;
        out.push_str(&tag[last..value_start]);
        out.push_str(&cmd_escape(&tag[value_start..value_end]));
        last = value_end;
        cursor = value_end + 1;
    }
    out.push_str(&tag[last..]);
    out
}

fn next_attr(tag: &str, from: usize) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for name in ["text", "show"] {
        let pattern = format!("{name}=\"");
        let mut search = from;
        while let Some(offset) = tag[search..].find(&pattern) {
            let start = search + offset;
            if start == 0 || !is_word_char(tag[..start].chars().next_back().unwrap_or(' ')) {
                match best {
                    Some((known, _)) if known <= start => {}
                    _ => best = Some((start, name.len())),
                }
                break;
            }
            search = start + 1;
        }
    }
    best
}

fn has_attr(tag: &str, name: &str) -> bool {
    next_attr_of(tag, name, 0).is_some()
}

fn attr_value(tag: &str, name: &str) -> Option<String> {
    let start = next_attr_of(tag, name, 0)?;
    let value_start = start + name.len() + 2;
    let close = tag[value_start..].find('"')?;
    Some(percent_decode(&tag[value_start..value_start + close]))
}

fn next_attr_of(tag: &str, name: &str, from: usize) -> Option<usize> {
    let pattern = format!("{name}=\"");
    let mut search = from;
    while let Some(offset) = tag[search..].find(&pattern) {
        let start = search + offset;
        if start == 0 || !is_word_char(tag[..start].chars().next_back().unwrap_or(' ')) {
            return Some(start);
        }
        search = start + 1;
    }
    None
}

fn inject_show(tag: &str, show: &str) -> String {
    let closing = tag.len().saturating_sub(1);
    let head = tag[..closing].trim_end();
    let head = match head.strip_suffix('/') {
        Some(stripped) => stripped.trim_end(),
        None => head,
    };
    let mut out = String::with_capacity(tag.len() + show.len() + 10);
    out.push_str(head);
    out.push_str(" show=\"");
    out.push_str(show);
    out.push('"');
    out.push_str(&tag[head.len()..]);
    out
}

pub fn cmd_escape(value: &str) -> String {
    if has_percent_escape(value) {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + value.len() / 2);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push_str("%20"),
            other => {
                out.push('%');
                out.push(hex_digit(other >> 4));
                out.push(hex_digit(other & 0x0f));
            }
        }
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        other => (b'A' + (other - 10)) as char,
    }
}

fn has_percent_escape(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while index + 2 < bytes.len() {
        if bytes[index] == b'%'
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            return true;
        }
        index += 1;
    }
    false
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex_value(bytes[index + 1]);
                let low = hex_value(bytes[index + 2]);
                match (high, low) {
                    (Some(high), Some(low)) => {
                        out.push(high * 16 + low);
                        index += 3;
                    }
                    _ => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn fit_images(content: &str, width: u32, height: u32) -> String {
    if (width == 0 && height == 0) || !content.contains("px") {
        return content.to_string();
    }
    let images = find_images(content);
    if images.is_empty() {
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len());
    let mut last = 0usize;
    let mut changed = false;
    for image in &images {
        let Some(alt) = clamp_alt(&image.alt, width, height) else {
            continue;
        };
        let alt_start = image.start + 2;
        out.push_str(&content[last..alt_start]);
        out.push_str(&alt);
        last = alt_start + image.alt.len();
        changed = true;
    }
    if !changed {
        return content.to_string();
    }
    out.push_str(&content[last..]);
    out
}

fn clamp_alt(alt: &str, width: u32, height: u32) -> Option<String> {
    let dims = find_dims(alt);
    if dims.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(alt.len() + 8);
    let mut last = 0usize;
    let mut changed = false;
    for dim in &dims {
        let (limited_width, limited_height) = limit_dims(dim.width, dim.height, width, height);
        if limited_width == dim.width && limited_height == dim.height {
            continue;
        }
        out.push_str(&alt[last..dim.start]);
        out.push('#');
        out.push_str(&limited_width.to_string());
        out.push_str("px #");
        out.push_str(&limited_height.to_string());
        out.push_str("px");
        last = dim.end;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&alt[last..]);
    Some(out)
}

struct Dim {
    start: usize,
    end: usize,
    width: u64,
    height: u64,
}

fn find_dims(alt: &str) -> Vec<Dim> {
    let bytes = alt.as_bytes();
    let mut dims = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'#' {
            if let Some((dim, next)) = parse_dim(alt, index) {
                dims.push(dim);
                index = next;
                continue;
            }
        }
        index += 1;
    }
    dims
}

fn parse_dim(alt: &str, start: usize) -> Option<(Dim, usize)> {
    let bytes = alt.as_bytes();
    let mut cursor = start + 1;
    cursor = skip_spaces(bytes, cursor);
    let (width, after_width) = parse_dimension(bytes, cursor)?;
    cursor = skip_spaces(bytes, after_width);
    if !alt[cursor..].starts_with("px") {
        return None;
    }
    cursor = skip_spaces(bytes, cursor + 2);
    if bytes.get(cursor) != Some(&b'#') {
        return None;
    }
    cursor = skip_spaces(bytes, cursor + 1);
    let (height, after_height) = parse_dimension(bytes, cursor)?;
    cursor = skip_spaces(bytes, after_height);
    if !alt[cursor..].starts_with("px") {
        return None;
    }
    cursor += 2;
    Some((
        Dim {
            start,
            end: cursor,
            width,
            height,
        },
        cursor,
    ))
}

fn skip_spaces(bytes: &[u8], start: usize) -> usize {
    let mut cursor = start;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn parse_dimension(bytes: &[u8], start: usize) -> Option<(u64, usize)> {
    let first = *bytes.get(start)?;
    if !(b'1'..=b'9').contains(&first) {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - start < 9 {
        cursor += 1;
    }
    let text = std::str::from_utf8(&bytes[start..cursor]).ok()?;
    Some((text.parse().ok()?, cursor))
}

fn limit_dims(width: u64, height: u64, box_width: u32, box_height: u32) -> (u64, u64) {
    let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else {
        return (width, height);
    };
    let (limited_width, limited_height) = ImageBox::new(box_width, box_height).limit(w, h);
    (u64::from(limited_width), u64::from(limited_height))
}

struct FoundImage {
    start: usize,
    end: usize,
    alt: String,
    source: String,
}

fn find_images(content: &str) -> Vec<FoundImage> {
    if !content.contains("![") {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut index = 0usize;
    while index < content.len() {
        match parse_image(content, index) {
            Some(image) => {
                index = image.end;
                found.push(image);
            }
            None => index += 1,
        }
    }
    found
}

fn parse_image(content: &str, start: usize) -> Option<FoundImage> {
    let bytes = content.as_bytes();
    if bytes.get(start) != Some(&b'!') || bytes.get(start + 1) != Some(&b'[') {
        return None;
    }
    let alt_start = start + 2;
    let mut cursor = alt_start;
    while cursor < bytes.len() && bytes[cursor] != b']' {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b']') || bytes.get(cursor + 1) != Some(&b'(') {
        return None;
    }
    let source_start = cursor + 2;
    let mut end = source_start;
    while end < bytes.len() && bytes[end] != b')' {
        end += 1;
    }
    if bytes.get(end) != Some(&b')') || end == source_start {
        return None;
    }
    Some(FoundImage {
        start,
        end: end + 1,
        alt: content[alt_start..cursor].to_string(),
        source: content[source_start..end].to_string(),
    })
}

async fn embed_images(content: &str, store: &dyn ImageStore) -> (String, Vec<PreparedImage>) {
    let found = find_images(content);
    if found.is_empty() {
        return (content.to_string(), Vec::new());
    }
    let mut resolved: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    let mut images: Vec<PreparedImage> = Vec::new();
    let mut out = String::with_capacity(content.len());
    let mut last = 0usize;
    for image in &found {
        out.push_str(&content[last..image.start]);
        last = image.end;
        let slot = match resolved.iter().position(|entry| entry.0 == image.source) {
            Some(position) => position,
            None => {
                let outcome = store.ensure(&image.source).await;
                let (url, failure) = match outcome {
                    Ok(url) if !url.is_empty() => (Some(url), None),
                    Ok(_) => (None, Some("图床返回空地址".to_string())),
                    Err(error) => (None, Some(error.to_string())),
                };
                resolved.push((image.source.clone(), url, failure));
                resolved.len() - 1
            }
        };
        let (_, url, failure) = &resolved[slot];
        let target = url.clone().unwrap_or_else(|| image.source.clone());
        out.push_str("![");
        out.push_str(&image.alt);
        out.push_str("](");
        out.push_str(&target);
        out.push(')');
        if images.iter().any(|known| known.source == image.source) {
            continue;
        }
        images.push(PreparedImage {
            alt: image.alt.clone(),
            source: image.source.clone(),
            url: target,
            stored: url.is_some(),
            failure: failure.clone(),
            filename: filename_for(&image.source),
            mime: mime_for(&image.source),
        });
    }
    out.push_str(&content[last..]);
    (out, images)
}

fn filename_for(url: &str) -> String {
    let trimmed = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or("");
    if name.rfind('.').is_some() {
        name.to_string()
    } else {
        String::new()
    }
}

fn mime_for(url: &str) -> String {
    let name = filename_for(url);
    let extension = match name.rsplit_once('.') {
        Some((_, extension)) => extension.to_ascii_lowercase(),
        None => return String::new(),
    };
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        _ => return String::new(),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Store {
        calls: Mutex<Vec<String>>,
        failed: Vec<&'static str>,
    }

    impl Store {
        fn new(failed: Vec<&'static str>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                failed,
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("锁").clone()
        }
    }

    impl ImageStore for Store {
        fn ensure(&self, url: &str) -> BoxFuture<'_, Result<String, ApiError>> {
            let failed = self.failed.contains(&url);
            let source = url.to_string();
            self.calls.lock().expect("锁").push(source.clone());
            Box::pin(async move {
                if failed {
                    Err(ApiError::upload("图床上传失败"))
                } else {
                    Ok(format!("https://cdn.example.com/{source}"))
                }
            })
        }
    }

    #[tokio::test]
    async fn at_signs_never_become_real_mentions() {
        let prepared = prepare("@everyone 早上好", None, 0).await;
        assert_eq!(
            prepared.content,
            format!("@{ZERO_WIDTH_SPACE}everyone 早上好")
        );
        assert_eq!(protect_at("hello@example.com"), "hello@example.com");
        assert_eq!(protect_at("a@b"), "a@b");
        assert_eq!(protect_at("前缀@"), "前缀@");
        assert_eq!(protect_at("@"), "@");
        assert_eq!(
            protect_at("@a @b"),
            format!("@{ZERO_WIDTH_SPACE}a @{ZERO_WIDTH_SPACE}b")
        );
        assert!(
            prepare("问问 @user 这个", None, 0)
                .await
                .content
                .contains('\u{200b}'),
            "裸 @openid 必须被打断, 否则平台会当成真艾特"
        );
    }

    #[tokio::test]
    async fn channel_mention_syntax_is_preserved() {
        let prepared = prepare("<@!U1> 你好 <@openid123>", None, 0).await;
        assert_eq!(prepared.content, "<@!U1> 你好 <@openid123>");
        assert!(
            !prepared.content.contains('\u{200b}'),
            "已有艾特语法不得改写"
        );
    }

    #[tokio::test]
    async fn show_attribute_is_always_emitted() {
        let prepared = prepare(r#"<qqbot-cmd-input text="/p" show="pgr菜单" />"#, None, 0).await;
        assert_eq!(
            prepared.content,
            r#"<qqbot-cmd-input text="%2Fp" show="pgr%E8%8F%9C%E5%8D%95" />"#
        );
        assert!(
            prepared.content.contains("show="),
            "show 必须无条件输出, 省略会让 iOS 客户端重复插入指令"
        );
    }

    #[tokio::test]
    async fn missing_show_is_filled_from_text() {
        let prepared = prepare(r#"<qqbot-cmd-input text="/p" />"#, None, 0).await;
        assert_eq!(
            prepared.content,
            r#"<qqbot-cmd-input text="%2Fp" show="%2Fp" />"#
        );
    }

    #[tokio::test]
    async fn percent_encoding_matches_go() {
        assert_eq!(cmd_escape("/p rand"), "%2Fp%20rand");
        assert_eq!(cmd_escape("pgr菜单"), "pgr%E8%8F%9C%E5%8D%95");
        assert_eq!(cmd_escape(r#"a"b"#), "a%22b");
        assert_eq!(cmd_escape("/签到"), "%2F%E7%AD%BE%E5%88%B0");
        assert_eq!(cmd_escape("/p 100%"), "%2Fp%20100%25");
        let prepared = prepare(
            r#"<qqbot-cmd-enter text="/签到" /> <qqbot-cmd-input text="/a" show="arc菜单" />"#,
            None,
            0,
        )
        .await;
        assert_eq!(
            prepared.content,
            r#"<qqbot-cmd-enter text="%2F%E7%AD%BE%E5%88%B0" /> <qqbot-cmd-input text="%2Fa" show="arc%E8%8F%9C%E5%8D%95" />"#
        );
    }

    #[tokio::test]
    async fn encoding_is_idempotent_and_spares_foreign_attributes() {
        let once = prepare(r#"<qqbot-cmd-input text="/p" show="pgr菜单" />"#, None, 0).await;
        let twice = prepare(&once.content, None, 0).await;
        assert_eq!(twice.content, once.content, "重试会再跑一遍, 编码必须幂等");
        let kept = prepare(
            r#"<qqbot-cmd-input text="/p" reference="false" /> <qqbot-at-everyone />"#,
            None,
            0,
        )
        .await;
        assert_eq!(
            kept.content,
            r#"<qqbot-cmd-input text="%2Fp" reference="false" show="%2Fp" /> <qqbot-at-everyone />"#
        );
        assert_eq!(
            encode_cmd_tags("## 标题\n普通文案 <@U1>"),
            "## 标题\n普通文案 <@U1>"
        );
    }

    #[tokio::test]
    async fn images_are_rewritten_through_the_store() {
        let store = Store::new(Vec::new());
        let prepared = prepare(
            "前 ![图](https://origin.example.com/a.png) 后",
            Some(&store),
            0,
        )
        .await;
        assert_eq!(
            prepared.content,
            "前 ![图](https://cdn.example.com/https://origin.example.com/a.png) 后"
        );
        assert_eq!(prepared.images.len(), 1);
        let image = &prepared.images[0];
        assert_eq!(image.source, "https://origin.example.com/a.png");
        assert!(image.stored, "成功图床化的图要标记为已转存");
        assert_eq!(image.failure, None);
        assert_eq!(image.filename, "a.png");
        assert_eq!(image.mime, "image/png");
        assert!(!image.byte_direct(), "转存成功不需要退字节直传");
    }

    #[tokio::test]
    async fn failed_images_degrade_to_the_original_url() {
        let store = Store::new(vec!["https://origin.example.com/broken.png"]);
        let prepared = prepare(
            "![坏](https://origin.example.com/broken.png)",
            Some(&store),
            0,
        )
        .await;
        assert_eq!(
            prepared.content,
            "![坏](https://origin.example.com/broken.png)"
        );
        let image = &prepared.images[0];
        assert!(!image.stored, "转存失败要标记为未转存");
        assert!(image.byte_direct(), "频道场景要靠这条信息退字节直传");
        assert!(
            image.failure.as_deref().unwrap_or("").contains("上传失败"),
            "失败原因要能带回去"
        );
        assert_eq!(image.filename, "broken.png");
        let unknown = prepare(
            "![x](https://origin.example.com/no-extension)",
            Some(&store),
            0,
        )
        .await;
        assert_eq!(unknown.images.len(), 1, "没有后缀的地址也算图片");
        assert!(
            unknown.images[0].filename.is_empty() && unknown.images[0].mime.is_empty(),
            "推断不出后缀时 filename/mime 留空, 不要编一个假的"
        );
    }

    #[tokio::test]
    async fn image_order_is_stable_and_duplicates_run_once() {
        let store = Store::new(Vec::new());
        let prepared = prepare(
            "![一](https://e.com/1.png) ![二](https://e.com/2.jpg) ![一](https://e.com/1.png)",
            Some(&store),
            0,
        )
        .await;
        let sources: Vec<&str> = prepared.images.iter().map(|i| i.source.as_str()).collect();
        assert_eq!(
            sources,
            vec!["https://e.com/1.png", "https://e.com/2.jpg"],
            "图片顺序要与正文出现顺序一致, 重复图只登记一次"
        );
        assert_eq!(
            store.calls(),
            vec![
                "https://e.com/1.png".to_string(),
                "https://e.com/2.jpg".to_string()
            ],
            "同一个地址只走一次图床"
        );
        assert_eq!(
            prepared
                .content
                .matches("https://cdn.example.com/https://e.com/1.png")
                .count(),
            2,
            "重复出现的图两处都要替换"
        );
        assert_eq!(prepared.images[1].mime, "image/jpeg");
    }

    #[tokio::test]
    async fn images_without_a_store_are_left_alone() {
        let source = "![图](https://e.com/a.png)";
        let prepared = prepare(source, None, 0).await;
        assert_eq!(prepared.content, source);
        assert!(prepared.images.is_empty(), "没有图床就不做图床管线");
    }

    #[tokio::test]
    async fn truncated_bodies_keep_utf8_intact() {
        let long = "汉".repeat(200);
        let prepared = prepare(&long, None, 50).await;
        assert!(prepared.truncated, "超出上限要报告截断");
        assert_eq!(prepared.content.chars().count(), 50);
        assert!(prepared.content.ends_with('…'), "截断末尾补省略号");
        assert_eq!(
            prepared.content.len(),
            49 * 3 + '…'.len_utf8(),
            "截断必须落在字符边界"
        );
        let short = prepare("短句", None, 50).await;
        assert!(!short.truncated, "未超上限不得标记截断");
        assert_eq!(short.content, "短句");
    }

    #[tokio::test]
    async fn zero_limit_means_no_truncation() {
        let long = "汉".repeat(5000);
        let prepared = prepare(&long, None, 0).await;
        assert!(!prepared.truncated, "limit=0 视为不限长");
        assert_eq!(prepared.content.chars().count(), 5000);
        let single = prepare("abcdef", None, 1).await;
        assert_eq!(single.content, "a", "limit=1 时只留一个 rune, 不加省略号");
        assert!(single.truncated);
    }

    #[tokio::test]
    async fn empty_and_plain_bodies_are_returned_verbatim() {
        let empty = prepare("", None, 16).await;
        assert_eq!(empty.content, "");
        assert!(empty.images.is_empty());
        assert!(!empty.truncated);
        let plain = "## 标题\n正文一行\n没有图床也没有指令标签";
        let prepared = prepare(plain, None, 0).await;
        assert_eq!(prepared.content, plain, "纯文本必须原样返回");
    }

    #[tokio::test]
    async fn latex_is_only_stripped_for_guild_text() {
        let source = "公式 $\\color{red}{\\frac{a}{b}} = c$ 结束";
        assert_eq!(strip_latex(source), "公式 ab = c 结束");
        assert_eq!(
            prepare(source, None, 0).await.content,
            source,
            "markdown 正文保留 LaTeX, 只有频道降级才剥"
        );
    }

    #[tokio::test]
    async fn image_dimensions_are_clamped_to_the_box() {
        assert_eq!(
            fit_images("![B30 #1500px #3219px](https://e/a.png)", 350, 438),
            "![B30 #350px #438px](https://e/a.png)"
        );
        assert_eq!(
            fit_images(
                "## ![cornerRadius=100 #30px #30px](https://e/avatar)",
                350,
                438
            ),
            "## ![cornerRadius=100 #30px #30px](https://e/avatar)",
            "小图只缩不放"
        );
        assert_eq!(
            fit_images("![图](https://e/a.png)", 350, 438),
            "![图](https://e/a.png)",
            "没标尺寸的图原样保留"
        );
        assert_eq!(
            fit_images("![图 #0px #0px](https://e/a.png)", 350, 438),
            "![图 #0px #0px](https://e/a.png)",
            "零尺寸不参与夹取"
        );
        assert_eq!(
            fit_images("![x #100px #9999px](https://e/a.png)", 0, 0),
            "![x #100px #9999px](https://e/a.png)",
            "上限为 0 表示不生效"
        );
        let prepared = prepare_with(
            "![B30 #1500px #3219px](https://e/a.png)",
            None,
            0,
            Some((350, 438)),
        )
        .await;
        assert_eq!(prepared.content, "![B30 #350px #438px](https://e/a.png)");
    }
}
