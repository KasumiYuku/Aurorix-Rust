pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&#34;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_the_five_go_entities() {
        assert_eq!(
            escape_html("<script>alert(\"x&y\")</script>"),
            "&lt;script&gt;alert(&#34;x&amp;y&#34;)&lt;/script&gt;"
        );
        assert_eq!(escape_html("it's"), "it&#39;s");
        assert_eq!(escape_html("甲"), "甲");
    }
}
