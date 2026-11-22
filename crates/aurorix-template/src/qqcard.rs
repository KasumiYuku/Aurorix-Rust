use std::collections::BTreeMap;

use crate::args::FlatArgs;
use crate::error::{TemplateError, TemplateResult};
use crate::url_codec::encode_value;

pub const QQ_CARD_FUNC: &str = "qqcard";

pub fn qq_card_params() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut map = BTreeMap::new();
    map.insert(
        "1",
        vec![
            "leftImageUrl",
            "leftImageWidth",
            "leftImageHeight",
            "scheme",
            "text1",
            "text2",
            "text3",
            "icon3",
        ],
    );
    map.insert(
        "2",
        vec![
            "leftImageUrl",
            "scheme",
            "text1",
            "text2",
            "text3",
            "icon1",
            "icon2",
            "icon3",
            "iconDye1",
            "iconDye2",
            "iconDye3",
        ],
    );
    map.insert("3", vec!["title", "desc", "tag1", "tag2", "tag3"]);
    map.insert("5", vec!["text1", "text2", "plot_id"]);
    map
}

pub fn card_styles() -> Vec<&'static str> {
    let mut styles: Vec<&'static str> = qq_card_params().into_keys().collect();
    styles.sort_unstable();
    styles
}

pub fn is_qqcard_call(tag: &str) -> bool {
    qq_card_call_body(tag).is_some()
}

pub fn qq_card_call_body(tag: &str) -> Option<&str> {
    if tag == QQ_CARD_FUNC {
        return Some("");
    }
    let rest = tag.strip_prefix(QQ_CARD_FUNC)?;
    if !rest.starts_with(is_space_byte) {
        return None;
    }
    Some(rest)
}

fn is_space_byte(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedArg {
    pub key: String,
    pub value: String,
    pub literal: bool,
}

pub fn qq_card_arg_names(tag: &str) -> Vec<String> {
    let Some(body) = qq_card_call_body(tag) else {
        return Vec::new();
    };
    let Ok((_, rest)) = split_leading_token(body) else {
        return Vec::new();
    };
    let Ok(args) = parse_named_args(rest) else {
        return Vec::new();
    };
    args.into_iter()
        .filter(|arg| !arg.literal)
        .map(|arg| arg.value)
        .collect()
}

pub fn render_qq_card(tag: &str, flat: &FlatArgs) -> TemplateResult<String> {
    let Some(body) = qq_card_call_body(tag) else {
        return Err(TemplateError::NotQQCardCall(tag.to_string()));
    };
    let (style, rest) = split_leading_token(body).map_err(|_| TemplateError::QQCardMissingStyle)?;
    let params = qq_card_params();
    let Some(names) = params.get(style.as_str()) else {
        return Err(TemplateError::QQCardUnknownStyle {
            style: style.clone(),
            available: card_styles().join(" "),
        });
    };
    let args =
        parse_named_args(rest).map_err(|err| TemplateError::QQCardMalformed(err.to_string()))?;
    let mut pairs = vec![format!("id={style}")];
    for arg in args {
        if !names.contains(&arg.key.as_str()) {
            return Err(TemplateError::QQCardUnknownParam {
                style: style.clone(),
                key: arg.key.clone(),
                available: names.join(" "),
            });
        }
        let value = if arg.literal {
            arg.value.clone()
        } else {
            match flat.get(&arg.value) {
                Some(found) => found.clone(),
                None => {
                    return Err(TemplateError::QQCardArgNotFound {
                        key: arg.key.clone(),
                        value: arg.value.clone(),
                    });
                }
            }
        };
        if value.trim().is_empty() {
            continue;
        }
        pairs.push(format!("{}={}", arg.key, encode_value(&value)));
    }
    Ok(format!("<mqqapi://markdown/template?{}>", pairs.join("&")))
}

pub fn split_leading_token(body: &str) -> TemplateResult<(String, &str)> {
    let body = body.trim_start();
    if body.is_empty() {
        return Err(TemplateError::MissingLeadingToken);
    }
    match body.find(char::is_whitespace) {
        Some(at) => Ok((body[..at].to_string(), &body[at..])),
        None => Ok((body.to_string(), "")),
    }
}

pub fn parse_named_args(input: &str) -> TemplateResult<Vec<NamedArg>> {
    let mut out = Vec::new();
    let mut index = 0usize;
    loop {
        while index < input.len() && input.is_char_boundary(index) {
            let current = input[index..].chars().next().expect("cursor character");
            if !is_space_byte(current) {
                break;
            }
            index += current.len_utf8();
        }
        if index >= input.len() {
            return Ok(out);
        }
        let start = index;
        while index < input.len() {
            let current = input[index..].chars().next().expect("cursor character");
            if current == '=' || is_space_byte(current) {
                break;
            }
            index += current.len_utf8();
        }
        if index >= input.len() || !input[index..].starts_with('=') {
            return Err(TemplateError::NamedArgMissingEquals(
                input[start..].to_string(),
            ));
        }
        let key = input[start..index].to_string();
        if key.is_empty() {
            return Err(TemplateError::NamedArgMissingName);
        }
        let (value, next, literal) = read_named_value(input, index + 1)?;
        out.push(NamedArg {
            key,
            value,
            literal,
        });
        index = next;
    }
}

fn read_named_value(input: &str, start: usize) -> TemplateResult<(String, usize, bool)> {
    let Some(first) = input[start..].chars().next() else {
        return Ok((String::new(), start, false));
    };
    if first == '"' || first == '\'' {
        let mut out = String::new();
        let mut index = start + first.len_utf8();
        while index < input.len() {
            let current = input[index..].chars().next().expect("cursor character");
            if current == first {
                return Ok((out, index + current.len_utf8(), true));
            }
            out.push(current);
            index += current.len_utf8();
        }
        return Err(TemplateError::NamedArgUnclosedQuote);
    }
    let mut index = start;
    while index < input.len() {
        let current = input[index..].chars().next().expect("cursor character");
        if is_space_byte(current) {
            break;
        }
        index += current.len_utf8();
    }
    Ok((input[start..index].to_string(), index, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(pairs: &[(&str, &str)]) -> FlatArgs {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn renders_card_two_as_one_line() {
        let rendered = render_qq_card(
            "qqcard 2 leftImageUrl=cover scheme=link text1=title text2=artist",
            &flat(&[
                ("cover", "https://y.gtimg.cn/cover.jpg"),
                ("link", "https://y.qq.com/n/ryqq/songDetail/abc"),
                ("title", "江湖少年"),
                ("artist", "封茗囧菌,双笙"),
            ]),
        )
        .unwrap();
        assert_eq!(
            rendered,
            "<mqqapi://markdown/template?id=2&leftImageUrl=https%3A%2F%2Fy.gtimg.cn%2Fcover.jpg&scheme=https%3A%2F%2Fy.qq.com%2Fn%2Fryqq%2FsongDetail%2Fabc&text1=%E6%B1%9F%E6%B9%96%E5%B0%91%E5%B9%B4&text2=%E5%B0%81%E8%8C%97%E5%9B%A7%E8%8F%8C%2C%E5%8F%8C%E7%AC%99>"
        );
    }

    #[test]
    fn escapes_ampersand_inside_values() {
        let rendered =
            render_qq_card("qqcard 2 text1=title", &flat(&[("title", "标题&副标题")])).unwrap();
        assert_eq!(
            rendered,
            "<mqqapi://markdown/template?id=2&text1=%E6%A0%87%E9%A2%98%26%E5%89%AF%E6%A0%87%E9%A2%98>"
        );
    }

    #[test]
    fn accepts_quoted_literals() {
        let rendered = render_qq_card(
            r#"qqcard 3 title=title desc="来自 点歌""#,
            &flat(&[("title", "标题")]),
        )
        .unwrap();
        assert_eq!(
            rendered,
            "<mqqapi://markdown/template?id=3&title=%E6%A0%87%E9%A2%98&desc=%E6%9D%A5%E8%87%AA%20%E7%82%B9%E6%AD%8C>"
        );
    }

    #[test]
    fn omits_empty_values_entirely() {
        let rendered = render_qq_card(
            "qqcard 2 leftImageUrl=cover scheme=link text1=title text2=artist",
            &flat(&[
                ("cover", ""),
                ("link", "  "),
                ("title", "只有标题"),
                ("artist", ""),
            ]),
        )
        .unwrap();
        assert_eq!(
            rendered,
            "<mqqapi://markdown/template?id=2&text1=%E5%8F%AA%E6%9C%89%E6%A0%87%E9%A2%98>"
        );
    }

    #[test]
    fn rejects_bad_calls() {
        assert_eq!(
            render_qq_card("qqcard 9 text1=a", &FlatArgs::new()).unwrap_err(),
            TemplateError::QQCardUnknownStyle {
                style: "9".to_string(),
                available: "1 2 3 5".to_string(),
            }
        );
        assert!(matches!(
            render_qq_card(
                r#"qqcard 5 text1="a" text2="b" plot_id="c" nope="d""#,
                &FlatArgs::new()
            )
            .unwrap_err(),
            TemplateError::QQCardUnknownParam { .. }
        ));
        assert_eq!(
            render_qq_card("qqcard 5 text1=a", &FlatArgs::new()).unwrap_err(),
            TemplateError::QQCardArgNotFound {
                key: "text1".to_string(),
                value: "a".to_string()
            }
        );
        assert_eq!(
            render_qq_card("qqcard", &FlatArgs::new()).unwrap_err(),
            TemplateError::QQCardMissingStyle
        );
        assert!(matches!(
            render_qq_card("qqcard 5 text1 a", &FlatArgs::new()).unwrap_err(),
            TemplateError::QQCardMalformed(_)
        ));
    }

    #[test]
    fn missing_template_arg_is_reported() {
        let err =
            render_qq_card("qqcard 2 text1=nowhere", &flat(&[("title", "标题")])).unwrap_err();
        assert_eq!(
            err,
            TemplateError::QQCardArgNotFound {
                key: "text1".to_string(),
                value: "nowhere".to_string(),
            }
        );
    }

    #[test]
    fn collects_only_dynamic_arg_names() {
        assert_eq!(
            qq_card_arg_names("qqcard 2 leftImageUrl=cover scheme=link text1=title").len(),
            3
        );
        assert_eq!(
            qq_card_arg_names(r#"qqcard 3 title="写死的标题" desc=role"#),
            vec!["role".to_string()]
        );
        assert_eq!(qq_card_arg_names("qqcard 5 text1=a"), vec!["a".to_string()]);
    }

    #[test]
    fn style_detection_uses_byte_space_only() {
        assert!(is_qqcard_call("qqcard 2"));
        assert!(is_qqcard_call("qqcard"));
        assert!(!is_qqcard_call("qqcardx"));
        assert!(!is_qqcard_call("qqcards 2"));
    }
}
