pub const GUILD_TEXT_LIMIT: usize = 2000;

const CMD_TAG_PREFIX: &str = "<qqbot-cmd-";
const AT_EVERYONE_TAG: &str = "<qqbot-at-everyone />";
const AT_EVERYONE_TEXT: &str = "@全体成员";

pub fn split_images(content: &str) -> (Vec<String>, String) {
    if !content.contains("![") {
        return (Vec::new(), content.to_string());
    }
    let chars: Vec<char> = content.chars().collect();
    let mut urls = Vec::new();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '!' && chars.get(index + 1) == Some(&'[') {
            if let Some((end, url)) = parse_image_at(&chars, index) {
                urls.push(url);
                index = end;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    (urls, out)
}

fn parse_image_at(chars: &[char], start: usize) -> Option<(usize, String)> {
    let mut cursor = start + 2;
    while cursor < chars.len() && chars[cursor] != ']' {
        if chars[cursor] == '\n' {
            return None;
        }
        cursor += 1;
    }
    if chars.get(cursor) != Some(&']') || chars.get(cursor + 1) != Some(&'(') {
        return None;
    }
    let url_start = cursor + 2;
    let mut url_end = url_start;
    while url_end < chars.len() && chars[url_end] != ')' {
        if chars[url_end].is_whitespace() {
            return None;
        }
        url_end += 1;
    }
    if chars.get(url_end) != Some(&')') {
        return None;
    }
    let url: String = chars[url_start..url_end].iter().collect();
    if url.is_empty() {
        return None;
    }
    Some((url_end + 1, url))
}

pub fn guild_text(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let mut text = render_cmd_tags(content);
    text = text.replace(AT_EVERYONE_TAG, AT_EVERYONE_TEXT);
    text = drop_fenced_lines(&text);
    text = strip_inline_code(&text);
    text = strip_line_prefix(&text, PrefixKind::Heading);
    text = strip_line_prefix(&text, PrefixKind::Quote);
    text = strip_bold(&text);
    text = strip_latex(&text);
    let (_, mut text) = split_images(&text);
    text = strip_links(&text);
    text = strip_bare_urls(&text);
    text = drop_horizontal_rules(&text);
    truncate_runes(&tidy_guild_lines(&text), GUILD_TEXT_LIMIT)
}

pub fn tidy_guild_lines(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() || decorative(trimmed) {
            continue;
        }
        kept.push(trimmed);
    }
    kept.join("\n")
}

pub fn decorative(line: &str) -> bool {
    !line.chars().any(|c| c.is_alphanumeric())
}

pub fn truncate_runes(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    if text.chars().count() <= limit {
        return text.to_string();
    }
    if limit <= 1 {
        return text.chars().take(limit).collect();
    }
    let mut out: String = text.chars().take(limit - 1).collect();
    out.push('…');
    out
}

pub fn render_cmd_tags(text: &str) -> String {
    if !text.contains(CMD_TAG_PREFIX) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if starts_with_at(&chars, index, CMD_TAG_PREFIX) {
            if let Some(tag_end) = find_tag_end(&chars, index) {
                let tag: String = chars[index..tag_end].iter().collect();
                out.push_str(&cmd_tag_text(&tag));
                index = tag_end;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn starts_with_at(chars: &[char], start: usize, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    if start + needle.len() > chars.len() {
        return false;
    }
    chars[start..start + needle.len()] == needle[..]
}

fn find_tag_end(chars: &[char], start: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < chars.len() {
        if chars[cursor] == '>' {
            return Some(cursor + 1);
        }
        if chars[cursor] == '\n' {
            return None;
        }
        cursor += 1;
    }
    None
}

fn cmd_tag_text(tag: &str) -> String {
    let name_chars: Vec<char> = tag.chars().collect();
    let prefix_len = CMD_TAG_PREFIX.chars().count();
    let mut cursor = prefix_len;
    let mut name = String::new();
    while cursor < name_chars.len() {
        let c = name_chars[cursor];
        if c.is_ascii_alphabetic() {
            name.push(c);
            cursor += 1;
        } else {
            break;
        }
    }
    if name != "input" && name != "enter" {
        return tag.to_string();
    }
    let show = cmd_attr(tag, "show");
    if show.starts_with('/') {
        return show;
    }
    let text = cmd_attr(tag, "text");
    if !text.is_empty() {
        return text;
    }
    show
}

fn cmd_attr(tag: &str, name: &str) -> String {
    let key = format!("{name}=\"");
    let Some(start) = tag.find(&key) else {
        return String::new();
    };
    let rest = &tag[start + key.len()..];
    let Some(end) = rest.find('"') else {
        return String::new();
    };
    percent_decode(&rest[..end])
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

fn drop_fenced_lines(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        if line.trim_start().starts_with("```") {
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

fn strip_inline_code(text: &str) -> String {
    if !text.contains('`') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '`' {
            let mut end = index + 1;
            let mut closed = false;
            while end < chars.len() {
                if chars[end] == '\n' || chars[end] == '\\' {
                    break;
                }
                if chars[end] == '`' {
                    closed = true;
                    break;
                }
                end += 1;
            }
            if closed {
                out.extend(chars[index + 1..end].iter());
                index = end + 1;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

enum PrefixKind {
    Heading,
    Quote,
}

fn strip_line_prefix(text: &str, kind: PrefixKind) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        let mut cursor = 0usize;
        while cursor < chars.len() && (chars[cursor] == ' ' || chars[cursor] == '\t') {
            cursor += 1;
        }
        let mut end = cursor;
        match kind {
            PrefixKind::Heading => {
                let mut hashes = 0usize;
                while end < chars.len() && chars[end] == '#' && hashes < 6 {
                    hashes += 1;
                    end += 1;
                }
                if hashes > 0 && chars.get(end) == Some(&' ') {
                    end += 1;
                } else {
                    end = cursor;
                }
            }
            PrefixKind::Quote => {
                if chars.get(end) == Some(&'>') {
                    end += 1;
                    if chars.get(end) == Some(&' ') {
                        end += 1;
                    }
                }
            }
        }
        let rest: String = chars[end..].iter().collect();
        out.push(rest);
    }
    out.join("\n")
}

fn strip_bold(text: &str) -> String {
    text.replace("**", "").replace("__", "")
}

fn strip_links(text: &str) -> String {
    if !text.contains('[') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '[' {
            if let Some((end, label)) = parse_link_at(&chars, index) {
                out.push_str(&label);
                index = end;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn parse_link_at(chars: &[char], start: usize) -> Option<(usize, String)> {
    let mut cursor = start + 1;
    while cursor < chars.len() && chars[cursor] != ']' {
        if chars[cursor] == '\n' {
            return None;
        }
        cursor += 1;
    }
    if chars.get(cursor) != Some(&']') || chars.get(cursor + 1) != Some(&'(') {
        return None;
    }
    let label: String = chars[start + 1..cursor].iter().collect();
    if label.contains('[') {
        return None;
    }
    let mut url_end = cursor + 2;
    while url_end < chars.len() && chars[url_end] != ')' {
        if chars[url_end].is_whitespace() {
            return None;
        }
        url_end += 1;
    }
    if chars.get(url_end) != Some(&')') {
        return None;
    }
    Some((url_end + 1, label))
}

fn strip_bare_urls(text: &str) -> String {
    if !text.contains("http") {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        let rest = index + 4;
        let is_http = starts_with_at(&chars, index, "http");
        if is_http {
            let mut scheme_end = rest;
            let mut scheme = String::new();
            while scheme_end < chars.len() && chars[scheme_end].is_ascii_alphabetic() {
                scheme.push(chars[scheme_end]);
                scheme_end += 1;
            }
            if (scheme == "s" || scheme.is_empty()) && starts_with_at(&chars, scheme_end, "://") {
                let mut cursor = scheme_end + 3;
                while cursor < chars.len() && !chars[cursor].is_whitespace() {
                    cursor += 1;
                }
                index = cursor;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn drop_horizontal_rules(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        let trimmed = line.trim();
        if is_rule(trimmed) {
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

fn is_rule(line: &str) -> bool {
    if line.chars().count() < 3 {
        return false;
    }
    let first = match line.chars().next() {
        Some(c) => c,
        None => return false,
    };
    if first != '-' && first != '*' && first != '_' {
        return false;
    }
    line.chars().all(|c| c == first)
}

pub fn strip_latex(text: &str) -> String {
    if !text.contains('$') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '$' {
            let mut run = 1usize;
            while index + run < chars.len() && chars[index + run] == '$' && run < 2 {
                run += 1;
            }
            let mut cursor = index + run;
            let mut close = None;
            while cursor < chars.len() {
                if chars[cursor] == '$' {
                    let mut close_run = 1usize;
                    while cursor + close_run < chars.len()
                        && chars[cursor + close_run] == '$'
                        && close_run < run
                    {
                        close_run += 1;
                    }
                    close = Some((cursor, cursor + close_run));
                    break;
                }
                cursor += 1;
            }
            if let Some((frag_end, whole_end)) = close {
                let fragment: String = chars[index + run..frag_end].iter().collect();
                out.push_str(&clean_latex_fragment(&fragment));
                index = whole_end;
                continue;
            }
            out.push(chars[index]);
            index += 1;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn clean_latex_fragment(fragment: &str) -> String {
    let chars: Vec<char> = fragment.chars().collect();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        let c = chars[index];
        if c != '\\' {
            if c != '$' && c != '{' && c != '}' {
                out.push(c);
            }
            index += 1;
            continue;
        }
        index += 1;
        if index >= chars.len() {
            break;
        }
        let next = chars[index];
        if next == ';' || next == ',' || next == ':' || next == '!' || next == '>' {
            out.push(' ');
            index += 1;
            continue;
        }
        let name_start = index;
        while index < chars.len() && chars[index].is_alphabetic() {
            index += 1;
        }
        let name: String = chars[name_start..index].iter().collect();
        if name == "quad" || name == "qquad" {
            out.push(' ');
            continue;
        }
        if name == "color" {
            if chars.get(index) == Some(&'{') {
                let mut depth = 0i32;
                while index < chars.len() {
                    if chars[index] == '{' {
                        depth += 1;
                    } else if chars[index] == '}' {
                        depth -= 1;
                        if depth == 0 {
                            index += 1;
                            break;
                        }
                    }
                    index += 1;
                }
            }
            continue;
        }
        if chars.get(index) == Some(&'{') {
            index += 1;
        }
    }
    out.split_whitespace().collect::<Vec<&str>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_split_out_of_the_body() {
        let (urls, rest) = split_images("前 ![](https://img/a.png) 后");
        assert_eq!(urls, vec!["https://img/a.png".to_string()]);
        assert_eq!(rest, "前  后");
        let (local, rest) = split_images("x![](/opt/plugin/cache/pick.png)y");
        assert_eq!(
            local,
            vec!["/opt/plugin/cache/pick.png".to_string()],
            "本地渲染路径也要能摘出来"
        );
        assert_eq!(rest, "xy");
        let (none, same) = split_images("没有图片");
        assert!(none.is_empty());
        assert_eq!(same, "没有图片");
        let (broken, kept) = split_images("![alt](http://a/b c)");
        assert!(broken.is_empty(), "带空格的 URL 不算图片");
        assert_eq!(kept, "![alt](http://a/b c)");
    }

    #[test]
    fn urls_are_stripped_from_guild_text() {
        let text =
            guild_text("看这里 https://example.com/x.png 和 [锚点](https://example.com/y) 完");
        assert_eq!(text, "看这里  和 锚点 完");
        assert!(!text.contains("http"), "频道正文不能留裸链");
    }

    #[test]
    fn markdown_decoration_is_flattened() {
        let text = guild_text("# 标题\n> 引用\n**加粗** 与 `代码`\n---\n正文");
        assert_eq!(text, "标题\n引用\n加粗 与 代码\n正文");
    }

    #[test]
    fn separator_only_lines_are_dropped() {
        let text = guild_text("抽卡\n· · ·\n结果");
        assert_eq!(text, "抽卡\n结果");
        let dashes = guild_text("A\n- - -\nB");
        assert_eq!(dashes, "A\nB");
        assert!(decorative("···"));
        assert!(decorative("   "));
        assert!(!decorative("有字"));
        assert!(!decorative("123"));
    }

    #[test]
    fn fenced_code_lines_are_dropped() {
        let text = guild_text("前\n```rust\nlet a = 1;\n```\n后");
        assert_eq!(text, "前\nlet a = 1;\n后");
    }

    #[test]
    fn cmd_tags_become_copyable_commands() {
        let text = guild_text("<qqbot-cmd-input text=\"/pick 绘\" show=\"选曲\"/> 点这个");
        assert_eq!(text, "/pick 绘 点这个", "text 可照抄时优先用 text");
        let slash = render_cmd_tags("<qqbot-cmd-enter text=\"/help\" show=\"/帮助\"/>");
        assert_eq!(slash, "/帮助", "show 以斜杠开头时优先照抄 show");
        let encoded =
            render_cmd_tags("<qqbot-cmd-input text=\"%2Fpick+%E7%BB%98\" show=\"选曲\"/>");
        assert_eq!(encoded, "/pick 绘", "属性值要按百分号解码");
        assert_eq!(
            render_cmd_tags("<qqbot-cmd-other text=\"x\"/>"),
            "<qqbot-cmd-other text=\"x\"/>"
        );
        assert_eq!(guild_text("<qqbot-at-everyone /> 集合"), "@全体成员 集合");
    }

    #[test]
    fn long_bodies_are_truncated_by_rune() {
        let long = "汉".repeat(2500);
        let text = guild_text(&long);
        assert_eq!(
            text.chars().count(),
            GUILD_TEXT_LIMIT,
            "截断后正好 2000 rune"
        );
        assert!(text.ends_with('…'), "截断要补省略号");
        assert_eq!(truncate_runes("abc", 10), "abc");
        assert_eq!(truncate_runes("abcdef", 4), "abc…");
        assert_eq!(truncate_runes("abcdef", 1), "a");
        assert_eq!(truncate_runes("abcdef", 0), "");
    }

    #[test]
    fn latex_fragments_lose_commands() {
        let text = guild_text("公式 $\\color{red}{\\frac{a}{b}} = c$ 结束");
        assert_eq!(text, "公式 ab = c 结束");
    }

    #[test]
    fn blank_and_padded_lines_are_tidied() {
        assert_eq!(tidy_guild_lines("  a  \n\n   \nb\t"), "a\nb");
        assert_eq!(guild_text(""), "");
    }
}
