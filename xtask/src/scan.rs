#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    Line,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub kind: CommentKind,
    pub snippet: String,
}

const SNIPPET_LIMIT: usize = 60;

pub fn scan_source(src: &str) -> Vec<Violation> {
    let chars: Vec<char> = src.chars().collect();
    let mut violations = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            i += 1;
        } else if c == '"' {
            i = skip_string(&chars, i + 1);
        } else if c == '\'' {
            i = skip_char_or_lifetime(&chars, i);
        } else if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            violations.push(make_violation(&chars, i, CommentKind::Line));
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            violations.push(make_violation(&chars, i, CommentKind::Block));
            i = skip_block_comment(&chars, i);
        } else if let Some(quote) = raw_string_quote(&chars, i) {
            i = skip_raw_string(&chars, quote);
        } else if (c == 'b' || c == 'c') && i + 1 < chars.len() && chars[i + 1] == '"' {
            i = skip_string(&chars, i + 2);
        } else {
            i += 1;
        }
    }
    violations
}

fn make_violation(chars: &[char], start: usize, kind: CommentKind) -> Violation {
    let mut line = 1usize;
    let mut column = 1usize;
    for &c in &chars[..start] {
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    let mut snippet: String = chars[start..]
        .iter()
        .take_while(|&&c| c != '\n')
        .collect::<String>()
        .trim_end()
        .to_string();
    let mut cut = false;
    while snippet.chars().count() > SNIPPET_LIMIT {
        snippet.pop();
        cut = true;
    }
    if cut {
        snippet.push('…');
    }
    Violation {
        line,
        column,
        kind,
        snippet,
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn skip_string(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

fn skip_char_or_lifetime(chars: &[char], start: usize) -> usize {
    let body = start + 1;
    if body >= chars.len() {
        return chars.len();
    }
    if chars[body] == '\\' {
        let mut i = body + 2;
        while i < chars.len() {
            if chars[i] == '\'' {
                return i + 1;
            }
            i += 1;
        }
        return chars.len();
    }
    if body + 1 < chars.len() && chars[body + 1] == '\'' {
        return body + 2;
    }
    let mut i = body;
    while i < chars.len() && is_ident_char(chars[i]) {
        i += 1;
    }
    if i == body {
        start + 1
    } else {
        i
    }
}

fn skip_block_comment(chars: &[char], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            depth += 1;
            i += 2;
        } else if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    chars.len()
}

fn raw_string_quote(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && is_ident_char(chars[start - 1]) {
        return None;
    }
    let mut p = start;
    if chars[p] == 'b' || chars[p] == 'c' {
        p += 1;
    }
    if p >= chars.len() || chars[p] != 'r' {
        return None;
    }
    p += 1;
    while p < chars.len() && chars[p] == '#' {
        p += 1;
    }
    if p < chars.len() && chars[p] == '"' {
        Some(p)
    } else {
        None
    }
}

fn skip_raw_string(chars: &[char], quote: usize) -> usize {
    let mut hashes = 0usize;
    let mut p = quote;
    while p > 0 && chars[p - 1] == '#' {
        hashes += 1;
        p -= 1;
    }
    let mut i = quote + 1;
    while i < chars.len() {
        if chars[i] == '"' {
            let mut k = i + 1;
            let mut seen = 0usize;
            while seen < hashes && k < chars.len() && chars[k] == '#' {
                seen += 1;
                k += 1;
            }
            if seen == hashes {
                return k;
            }
        }
        i += 1;
    }
    chars.len()
}

#[cfg(test)]
mod tests {
    use super::{scan_source, CommentKind};

    #[test]
    fn line_comment_reported() {
        let src = "fn main() {}\n//注释\n";
        let found = scan_source(src);
        assert_eq!(found.len(), 1, "行注释应当只报一处");
        assert_eq!(found[0].line, 2, "行注释应当落在第二行");
        assert_eq!(found[0].kind, CommentKind::Line, "类型应当是行注释");
    }

    #[test]
    fn url_inside_string_ignored() {
        let src = "fn f() {\n    let url = \"https://example.com\";\n}\n";
        assert!(scan_source(src).is_empty(), "字符串里的双斜杠不算注释");
    }

    #[test]
    fn raw_string_ignored() {
        let src = "fn f() {\n    let s = r#\"a // b\"#;\n}\n";
        assert!(scan_source(src).is_empty(), "原始字符串里的双斜杠不算注释");
    }

    #[test]
    fn nested_block_comment_counts_once() {
        let src = "/* a /* b */ c */\nfn f() {}\n";
        let found = scan_source(src);
        assert_eq!(found.len(), 1, "嵌套块注释整块只算一处违规");
        assert_eq!(found[0].kind, CommentKind::Block, "类型应当是块注释");
        let after = "fn f() {}\n";
        assert!(
            scan_source(after).is_empty(),
            "块注释结束后的代码不应被误判"
        );
    }

    #[test]
    fn lifetime_ignored() {
        let src = "fn f() -> &'static str {\n    \"x\"\n}\n";
        assert!(scan_source(src).is_empty(), "生命周期不是字符字面量");
    }

    #[test]
    fn hash_char_literal_ignored() {
        let src = "fn f() -> char {\n    '#'\n}\n";
        assert!(scan_source(src).is_empty(), "井号字符字面量不是原始字符串");
    }

    #[test]
    fn byte_string_ignored() {
        let src = "fn f() {\n    let b = b\"a // b\";\n}\n";
        assert!(scan_source(src).is_empty(), "字节字符串里的双斜杠不算注释");
    }

    #[test]
    fn doc_comment_reported() {
        let src = "/// 文档\nfn f() {}\n";
        let found = scan_source(src);
        assert_eq!(found.len(), 1, "文档注释也算违规");
        assert_eq!(found[0].kind, CommentKind::Line, "文档注释属于行注释");
    }

    #[test]
    fn inner_attribute_marker_not_confused() {
        let src = "let a = 1;\nlet s = \"a /* b\";\nlet c = 2;\n";
        assert!(scan_source(src).is_empty(), "字符串里的块注释符号不算注释");
    }

    #[test]
    fn multi_hash_raw_string_ignored() {
        let src = "fn f() {\n    let s = r##\"a #// b\"##;\n}\n";
        assert!(
            scan_source(src).is_empty(),
            "多井号原始字符串里的斜杠不算注释"
        );
    }
}
