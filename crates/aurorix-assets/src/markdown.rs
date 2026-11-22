use crate::provider::ResolvedImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageRef<'a> {
    pub start: usize,
    pub end: usize,
    pub alt: &'a str,
    pub src: &'a str,
}

pub fn find_images(content: &str) -> Vec<ImageRef<'_>> {
    let bytes = content.as_bytes();
    let mut found = Vec::new();
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index] == b'!' && bytes[index + 1] == b'[' {
            if let Some(reference) = image_at(content, index) {
                found.push(reference);
                index = reference.end;
                continue;
            }
        }
        index += 1;
    }
    found
}

fn image_at(content: &str, start: usize) -> Option<ImageRef<'_>> {
    let bytes = content.as_bytes();
    let alt_start = start + 2;
    let alt_len = content.get(alt_start..)?.find(']')?;
    let alt_end = alt_start + alt_len;
    if bytes.get(alt_end + 1) != Some(&b'(') {
        return None;
    }
    let src_start = alt_end + 2;
    let src_len = content.get(src_start..)?.find(')')?;
    if src_len == 0 {
        return None;
    }
    let src_end = src_start + src_len;
    Some(ImageRef {
        start,
        end: src_end + 1,
        alt: content.get(alt_start..alt_end)?,
        src: content.get(src_start..src_end)?,
    })
}

pub fn has_dimensions(alt: &str) -> bool {
    let bytes = alt.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'#' {
            if let Some(after_first) = dimension_end(bytes, index) {
                let mut cursor = after_first;
                while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                if cursor < bytes.len()
                    && bytes[cursor] == b'#'
                    && dimension_end(bytes, cursor).is_some()
                {
                    return true;
                }
            }
        }
        index += 1;
    }
    false
}

fn dimension_end(bytes: &[u8], hash: usize) -> Option<usize> {
    let mut cursor = hash + 1;
    let digits_start = cursor;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        cursor += 1;
    }
    if cursor == digits_start {
        return None;
    }
    if bytes.get(cursor) != Some(&b'p') || bytes.get(cursor + 1) != Some(&b'x') {
        return None;
    }
    Some(cursor + 2)
}

pub fn render_image(alt: &str, src: &str, resolved: &ResolvedImage) -> String {
    if resolved.url == src {
        return format!("![{alt}]({src})");
    }
    if resolved.width > 0 && resolved.height > 0 && !has_dimensions(alt) {
        let width = resolved.width;
        let height = resolved.height;
        let url = &resolved.url;
        return format!("![{alt} #{width}px #{height}px]({url})");
    }
    let url = &resolved.url;
    format!("![{alt}]({url})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_basic_images() {
        let content = "a ![alt one](https://x/1.png) b ![](file:///tmp/2.png) c";
        let found = find_images(content);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].alt, "alt one");
        assert_eq!(found[0].src, "https://x/1.png");
        assert_eq!(
            &content[found[0].start..found[0].end],
            "![alt one](https://x/1.png)"
        );
        assert_eq!(found[1].alt, "");
        assert_eq!(found[1].src, "file:///tmp/2.png");
    }

    #[test]
    fn ignores_malformed_markdown() {
        assert!(find_images("![a]()").is_empty());
        assert!(find_images("![a] (b)").is_empty());
        assert!(find_images("![a](b").is_empty());
        assert!(find_images("[a](b)").is_empty());
        assert!(find_images("plain text").is_empty());
    }

    #[test]
    fn detects_existing_dimensions() {
        assert!(has_dimensions("alt #12px #34px"));
        assert!(has_dimensions("#1px#2px"));
        assert!(has_dimensions("#1px  #2px"));
        assert!(!has_dimensions("alt"));
        assert!(!has_dimensions("#1px"));
        assert!(!has_dimensions("#px #2px"));
        assert!(!has_dimensions("1px 2px"));
    }

    #[test]
    fn renders_with_and_without_size() {
        let resolved =
            ResolvedImage::uploaded("https://cdn/a.png", "sgame", false).with_size(12, 34);
        assert_eq!(
            render_image("alt", "file:///tmp/a.png", &resolved),
            "![alt #12px #34px](https://cdn/a.png)"
        );
        assert_eq!(
            render_image("alt #1px #2px", "file:///tmp/a.png", &resolved),
            "![alt #1px #2px](https://cdn/a.png)"
        );
        let no_size = ResolvedImage::uploaded("https://cdn/a.png", "sgame", false);
        assert_eq!(
            render_image("", "file:///tmp/a.png", &no_size),
            "![](https://cdn/a.png)"
        );
    }

    #[test]
    fn unchanged_url_keeps_source() {
        let resolved = ResolvedImage::passthrough("https://cdn/a.png");
        assert_eq!(
            render_image("alt", "https://cdn/a.png", &resolved),
            "![alt](https://cdn/a.png)"
        );
    }
}
