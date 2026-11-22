pub fn query_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push(hex_digit(other >> 4));
                out.push(hex_digit(other & 0x0f));
            }
        }
    }
    out
}

pub fn query_unescape(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return None;
                }
                let high = hex_value(bytes[index + 1])?;
                let low = hex_value(bytes[index + 2])?;
                out.push((high << 4) | low);
                index += 3;
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
    String::from_utf8(out).ok()
}

pub fn encode_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let decoded = query_unescape(value).unwrap_or_else(|| value.to_string());
    query_escape(&decoded).replace('+', "%20")
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        other => (b'A' + (other - 10)) as char,
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_matches_go_query_escape_with_space_fix() {
        assert_eq!(
            encode_value("https://y.gtimg.cn/cover.jpg"),
            "https%3A%2F%2Fy.gtimg.cn%2Fcover.jpg"
        );
        assert_eq!(
            encode_value("来自 点歌"),
            "%E6%9D%A5%E8%87%AA%20%E7%82%B9%E6%AD%8C"
        );
        assert_eq!(
            encode_value("标题&副标题"),
            "%E6%A0%87%E9%A2%98%26%E5%89%AF%E6%A0%87%E9%A2%98"
        );
        assert_eq!(encode_value("a+b"), "a%20b");
        assert_eq!(encode_value(""), "");
    }

    #[test]
    fn double_encoding_is_undone_first() {
        assert_eq!(encode_value("%E4%B8%AD"), "%E4%B8%AD");
        assert_eq!(encode_value("a%20b"), "a%20b");
        assert_eq!(encode_value("100%"), "100%25");
    }

    #[test]
    fn unescape_rejects_broken_sequences() {
        assert_eq!(query_unescape("a%2"), None);
        assert_eq!(query_unescape("a%zz"), None);
        assert_eq!(query_unescape("a+b").unwrap(), "a b");
    }

    #[test]
    fn unreserved_characters_survive() {
        assert_eq!(query_escape("-_.~Az09"), "-_.~Az09");
    }
}
