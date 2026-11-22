use std::sync::LazyLock;

use regex::Regex;

use crate::args::FlatArgs;
use crate::error::{TemplateError, TemplateResult};
use crate::qqcard::render_qq_card;
use crate::url_codec::encode_value;

pub const INLINE_COMMAND_FUNC: &str = "inline";

pub const INLINE_TEXT_LIMIT: usize = 100;

static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\{(.*?)\}\}").expect("inline placeholder regex"));

pub fn placeholder_regex() -> &'static Regex {
    &PLACEHOLDER
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operand {
    pub value: String,
    pub literal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineCall {
    pub label: Operand,
    pub command: Operand,
    pub reference: Option<Operand>,
}

pub fn is_inline_call(tag: &str) -> bool {
    inline_call_body(tag).is_some()
}

pub fn inline_call_body(tag: &str) -> Option<&str> {
    if tag == INLINE_COMMAND_FUNC {
        return Some("");
    }
    let rest = tag.strip_prefix(INLINE_COMMAND_FUNC)?;
    let next = rest.chars().next()?;
    if !next.is_whitespace() {
        return None;
    }
    Some(rest.trim())
}

pub fn inline_arg_names(tag: &str) -> Vec<String> {
    let Ok(call) = parse_inline_call(tag) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for operand in [&call.label, &call.command] {
        if !operand.literal {
            names.push(operand.value.clone());
        }
    }
    if let Some(reference) = &call.reference {
        if !reference.literal {
            names.push(reference.value.clone());
        }
    }
    names
}

pub fn parse_inline_call(tag: &str) -> TemplateResult<InlineCall> {
    let Some(body) = inline_call_body(tag) else {
        return Err(TemplateError::NotInlineCall(tag.to_string()));
    };
    let parts = parse_inline_operands(body)?;
    if parts.len() < 2 || parts.len() > 3 {
        return Err(TemplateError::InlineArgCount);
    }
    let mut iter = parts.into_iter();
    let (Some(label), Some(command)) = (iter.next(), iter.next()) else {
        return Err(TemplateError::InlineArgCount);
    };
    Ok(InlineCall {
        label,
        command,
        reference: iter.next(),
    })
}

pub fn render_inline_command(tag: &str, flat: &FlatArgs) -> TemplateResult<String> {
    let call = parse_inline_call(tag)?;
    let label = resolve_inline_operand(&call.label, flat)?;
    let command = resolve_inline_operand(&call.command, flat)?;
    if label.trim().is_empty() {
        return Err(TemplateError::InlineEmptyLabel);
    }
    if command.trim().is_empty() {
        return Err(TemplateError::InlineEmptyCommand);
    }
    let reference = match &call.reference {
        Some(operand) => inline_truthy(&resolve_inline_operand(operand, flat)?),
        None => false,
    };
    Ok(build_inline_command(&command, &label, reference))
}

pub fn build_inline_command(command: &str, label: &str, reference: bool) -> String {
    let mut out = format!(
        "<qqbot-cmd-input text=\"{}\" show=\"{}\"",
        encode_value(command),
        encode_value(label)
    );
    if reference {
        out.push_str(" reference=\"true\"");
    }
    out.push_str(" />");
    out
}

pub fn inline_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes"
    )
}

pub fn inline_length_exceeded(value: &str) -> bool {
    value.chars().count() > INLINE_TEXT_LIMIT
}

pub fn resolve_inline_operand(operand: &Operand, flat: &FlatArgs) -> TemplateResult<String> {
    if operand.literal {
        return Ok(operand.value.clone());
    }
    flat.get(&operand.value)
        .cloned()
        .ok_or_else(|| TemplateError::InlineArgNotFound(operand.value.clone()))
}

pub fn parse_inline_operands(input: &str) -> TemplateResult<Vec<Operand>> {
    let mut parts = Vec::new();
    let mut index = 0usize;
    while index < input.len() {
        while index < input.len() {
            let current = input[index..].chars().next().expect("cursor character");
            if !current.is_whitespace() {
                break;
            }
            index += current.len_utf8();
        }
        if index >= input.len() {
            break;
        }
        let current = input[index..].chars().next().expect("cursor character");
        if current == '"' || current == '\'' {
            let (value, next) = parse_quoted_operand(input, index, current)?;
            parts.push(Operand {
                value,
                literal: true,
            });
            index = next;
            continue;
        }
        let start = index;
        while index < input.len() {
            let current = input[index..].chars().next().expect("cursor character");
            if current.is_whitespace() {
                break;
            }
            index += current.len_utf8();
        }
        parts.push(Operand {
            value: input[start..index].to_string(),
            literal: false,
        });
    }
    Ok(parts)
}

fn parse_quoted_operand(input: &str, start: usize, quote: char) -> TemplateResult<(String, usize)> {
    let mut out = String::new();
    let mut index = start + quote.len_utf8();
    while index < input.len() {
        let current = input[index..].chars().next().expect("cursor character");
        if current == '\\' {
            index += current.len_utf8();
            if index >= input.len() {
                return Err(TemplateError::InlineDanglingEscape);
            }
            let escaped = input[index..].chars().next().expect("escaped character");
            out.push(escaped);
            index += escaped.len_utf8();
            continue;
        }
        if current == quote {
            return Ok((out, index + current.len_utf8()));
        }
        out.push(current);
        index += current.len_utf8();
    }
    Err(TemplateError::InlineQuotedUnclosed)
}

pub fn process_inline_commands(template: &str, flat: &FlatArgs) -> TemplateResult<String> {
    let mut outcome: Option<TemplateError> = None;
    let rendered = PLACEHOLDER.replace_all(template, |captures: &regex::Captures<'_>| {
        if outcome.is_some() {
            return captures[0].to_string();
        }
        let tag = captures[1].trim();
        if is_inline_call(tag) {
            return match render_inline_command(tag, flat) {
                Ok(text) => text,
                Err(err) => {
                    outcome = Some(err);
                    captures[0].to_string()
                }
            };
        }
        if crate::qqcard::is_qqcard_call(tag) {
            return match render_qq_card(tag, flat) {
                Ok(text) => text,
                Err(err) => {
                    outcome = Some(err);
                    captures[0].to_string()
                }
            };
        }
        captures[0].to_string()
    });
    match outcome {
        Some(err) => Err(err),
        None => Ok(rendered.into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn flat(pairs: &[(&str, &str)]) -> FlatArgs {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<String, String>>()
    }

    #[test]
    fn inline_always_emits_show_attribute() {
        let rendered =
            render_inline_command(r#"inline "点击领取猫娘" "嘿嘿""#, &FlatArgs::new()).unwrap();
        assert_eq!(
            rendered,
            "<qqbot-cmd-input text=\"%E5%98%BF%E5%98%BF\" show=\"%E7%82%B9%E5%87%BB%E9%A2%86%E5%8F%96%E7%8C%AB%E5%A8%98\" />"
        );
        assert!(rendered.contains(" show=\""));
    }

    #[test]
    fn inline_resolves_named_operands() {
        let flat = flat(&[("action.label", "点[我]"), ("action.command", "/p b30 #1")]);
        let rendered = render_inline_command("inline action.label action.command", &flat).unwrap();
        assert_eq!(
            rendered,
            "<qqbot-cmd-input text=\"%2Fp%20b30%20%231\" show=\"%E7%82%B9%5B%E6%88%91%5D\" />"
        );
    }

    #[test]
    fn inline_reference_flag_only_when_truthy() {
        for value in ["true", "TRUE", " 1 ", "yes"] {
            let tag = format!("inline \"引用\" \"/p me\" \"{value}\"");
            assert!(render_inline_command(&tag, &FlatArgs::new())
                .unwrap()
                .ends_with(" reference=\"true\" />"));
        }
        for value in ["false", "0", "no", ""] {
            let tag = format!("inline \"标签\" \"/p me\" \"{value}\"");
            assert!(!render_inline_command(&tag, &FlatArgs::new())
                .unwrap()
                .contains("reference"));
        }
    }

    #[test]
    fn inline_rejects_wrong_operand_count() {
        assert_eq!(
            render_inline_command(r#"inline "只有标签""#, &FlatArgs::new()).unwrap_err(),
            TemplateError::InlineArgCount
        );
        assert_eq!(
            render_inline_command(r#"inline "a" "b" "c" "d""#, &FlatArgs::new()).unwrap_err(),
            TemplateError::InlineArgCount
        );
    }

    #[test]
    fn inline_rejects_empty_operands() {
        assert_eq!(
            render_inline_command(r#"inline "" "/p b50""#, &FlatArgs::new()).unwrap_err(),
            TemplateError::InlineEmptyLabel
        );
        assert_eq!(
            render_inline_command(r#"inline "标签" """#, &FlatArgs::new()).unwrap_err(),
            TemplateError::InlineEmptyCommand
        );
    }

    #[test]
    fn inline_missing_arg_names_the_operand() {
        let err =
            render_inline_command("inline label command", &flat(&[("label", "点我")])).unwrap_err();
        assert_eq!(err.to_string(), "inline arg command not found");
    }

    #[test]
    fn inline_over_length_is_still_emitted() {
        let long = "a".repeat(INLINE_TEXT_LIMIT + 1);
        let rendered =
            render_inline_command(&format!("inline \"标签\" \"{long}\""), &FlatArgs::new())
                .unwrap();
        assert!(rendered.contains("qqbot-cmd-input"));
        assert!(inline_length_exceeded(&long));
        assert!(!inline_length_exceeded(&"a".repeat(INLINE_TEXT_LIMIT)));
    }

    #[test]
    fn inline_quoted_operands_support_both_quote_styles_and_escapes() {
        let rendered =
            render_inline_command(r#"inline '单引号' "/a\"b""#, &FlatArgs::new()).unwrap();
        assert_eq!(
            rendered,
            "<qqbot-cmd-input text=\"%2Fa%22b\" show=\"%E5%8D%95%E5%BC%95%E5%8F%B7\" />"
        );
        assert_eq!(
            parse_inline_operands(r#""abc"#).unwrap_err(),
            TemplateError::InlineQuotedUnclosed
        );
        assert_eq!(
            parse_inline_operands(r#""abc\"#).unwrap_err(),
            TemplateError::InlineDanglingEscape
        );
    }

    #[test]
    fn inline_operands_tolerate_surrounding_whitespace() {
        assert!(parse_inline_operands("   ").unwrap().is_empty());
        assert!(parse_inline_operands("").unwrap().is_empty());
        let parts = parse_inline_operands(" a \t ").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].value, "a");
        assert_eq!(parse_inline_operands(r#" "x" "#).unwrap()[0].value, "x");
    }

    #[test]
    fn inline_call_detection_matches_go() {
        assert!(is_inline_call("inline"));
        assert!(is_inline_call("inline x"));
        assert!(!is_inline_call("inlinex"));
        assert!(is_inline_call("inline\tx\t"));
        assert!(is_inline_call("inline\u{3000}"));
    }

    #[test]
    fn inline_arg_names_skip_literals() {
        assert_eq!(
            inline_arg_names(r#"inline action.label "写死" flag"#),
            vec!["action.label".to_string(), "flag".to_string()]
        );
        assert!(inline_arg_names("inline").is_empty());
    }
}
