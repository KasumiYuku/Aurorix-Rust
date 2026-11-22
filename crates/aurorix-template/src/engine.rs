use std::collections::BTreeSet;

use crate::args::{flatten, Arg, Args, FlatArgs};
use crate::error::{TemplateError, TemplateResult};
use crate::inline::{inline_arg_names, is_inline_call, placeholder_regex, process_inline_commands};
use crate::qqcard::{is_qqcard_call, qq_card_arg_names};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessedTemplate {
    pub body: String,
    pub args: Vec<String>,
}

pub fn process_template(input: &str) -> ProcessedTemplate {
    let mut args: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut in_block = false;
    let body = placeholder_regex()
        .replace_all(input, |captures: &regex::Captures<'_>| {
            let trimmed = captures[1].trim();
            if trimmed.starts_with("#each") {
                in_block = true;
            } else if trimmed == "/each" {
                in_block = false;
            } else if is_inline_call(trimmed) {
                if !in_block {
                    for name in inline_arg_names(trimmed) {
                        if seen.insert(name.clone()) {
                            args.push(name);
                        }
                    }
                }
            } else if is_qqcard_call(trimmed) {
                if !in_block {
                    for name in qq_card_arg_names(trimmed) {
                        if seen.insert(name.clone()) {
                            args.push(name);
                        }
                    }
                }
            } else if !trimmed.is_empty() && !in_block && seen.insert(trimmed.to_string()) {
                args.push(trimmed.to_string());
            }
            format!("{{{{{trimmed}}}}}")
        })
        .into_owned();
    ProcessedTemplate { body, args }
}

pub fn fill(body: &str, args: &Args) -> TemplateResult<String> {
    let flat = flatten(args)?;
    let mut text = process_each(body, args, &flat)?;
    text = process_inline_commands(&text, &flat)?;
    for (key, value) in &flat {
        text = text.replace(&format!("{{{{{key}}}}}"), value);
    }
    let leftover = process_template(&text).args;
    if !leftover.is_empty() {
        return Err(TemplateError::LostArgs(leftover.join(", ")));
    }
    Ok(text)
}

pub fn process_each(template: &str, args: &Args, flat: &FlatArgs) -> TemplateResult<String> {
    if !template.contains("{{#each") {
        return Ok(template.to_string());
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    loop {
        let Some(start) = rest.find("{{#each") else {
            out.push_str(rest);
            break;
        };
        let Some(head_end) = rest[start..].find("}}") else {
            return Err(TemplateError::InvalidEachTag(rest[start..].to_string()));
        };
        let head = &rest[start..start + head_end + 2];
        let Some(key) = head
            .strip_suffix("}}")
            .and_then(|rest| rest.strip_prefix("{{#each"))
        else {
            return Err(TemplateError::InvalidEachTag(head.to_string()));
        };
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(TemplateError::EachMissingKey);
        }
        let body_start = start + head.len();
        let Some(tail_index) = rest[body_start..].find("{{/each}}") else {
            return Err(TemplateError::EachMissingEnd(key));
        };
        let body = &rest[body_start..body_start + tail_index];
        if body.contains("{{#each") {
            return Err(TemplateError::EachNested(key));
        }
        let absent: Vec<Arg> = Vec::new();
        let items = match args.get(&key) {
            Some(Arg::List(items)) => items,
            None => &absent,
            Some(other) => {
                return Err(TemplateError::EachArgType {
                    key,
                    type_name: other.type_label().to_string(),
                });
            }
        };
        out.push_str(&rest[..start]);
        for (index, item) in items.iter().enumerate() {
            if !matches!(item, Arg::Map(_)) {
                return Err(TemplateError::EachItemType {
                    key,
                    index,
                    type_name: item.type_label().to_string(),
                });
            }
            let prefix = format!("{key}.#{index}.");
            let mut scope = flat.clone();
            let mut segment = body.to_string();
            for (flat_key, flat_value) in flat {
                if let Some(local) = flat_key.strip_prefix(&prefix) {
                    scope.insert(local.to_string(), flat_value.clone());
                    segment = segment.replace(&format!("{{{{{local}}}}}"), flat_value);
                }
            }
            segment = process_inline_commands(&segment, &scope)?;
            out.push_str(&segment);
        }
        rest = &rest[body_start + tail_index + "{{/each}}".len()..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn processing_trims_placeholders_and_collects_args() {
        let processed = process_template("### {{ title }} {{a}} {{a}} {{}}");
        assert_eq!(processed.body, "### {{title}} {{a}} {{a}} {{}}");
        assert_eq!(processed.args, vec!["title".to_string(), "a".to_string()]);
    }

    #[test]
    fn args_inside_each_blocks_are_not_collected() {
        let processed = process_template("{{#each rows}}{{label}}{{/each}}{{tail}}");
        assert_eq!(processed.args, vec!["tail".to_string()]);
    }

    #[test]
    fn fill_replaces_nested_keys() {
        let mut user = BTreeMap::new();
        user.insert("name".to_string(), Arg::from("阿明"));
        user.insert(
            "items".to_string(),
            Arg::List(vec![Arg::from("剑"), Arg::from("盾")]),
        );
        let mut source = Args::new();
        source.insert("user".to_string(), Arg::Map(user));
        source.insert("count".to_string(), Arg::from(2i64));
        let rendered = fill(
            "{{user.name}} 有 {{user.items.#0}} 和 {{user.items.#1}}，共 {{count}} 个",
            &source,
        )
        .unwrap();
        assert_eq!(rendered, "阿明 有 剑 和 盾，共 2 个");
    }

    #[test]
    fn fill_reports_lost_args() {
        let err = fill("你好 {{name}}", &Args::new()).unwrap_err();
        assert_eq!(err, TemplateError::LostArgs("name".to_string()));
        assert_eq!(err.to_string(), "Lost args: name");
    }

    #[test]
    fn each_expands_rows_in_order() {
        let mut row_a = BTreeMap::new();
        row_a.insert("label".to_string(), Arg::from("A"));
        let mut row_b = BTreeMap::new();
        row_b.insert("label".to_string(), Arg::from("B"));
        let mut source = Args::new();
        source.insert(
            "rows".to_string(),
            Arg::List(vec![Arg::Map(row_a), Arg::Map(row_b)]),
        );
        let rendered = fill(
            "<ul>{{#each rows}}<li>{{label}}</li>{{/each}}</ul>",
            &source,
        )
        .unwrap();
        assert_eq!(rendered, "<ul><li>A</li><li>B</li></ul>");
    }

    #[test]
    fn each_rejects_malformed_blocks() {
        let mut source = Args::new();
        source.insert("rows".to_string(), Arg::List(vec![]));
        assert_eq!(
            fill("{{#each rows}}x", &source).unwrap_err(),
            TemplateError::EachMissingEnd("rows".to_string())
        );
        assert_eq!(
            fill("{{#each }}x{{/each}}", &source).unwrap_err(),
            TemplateError::EachMissingKey
        );
        assert_eq!(
            fill("{{#each rows}}{{#each rows}}x{{/each}}{{/each}}", &source).unwrap_err(),
            TemplateError::EachNested("rows".to_string())
        );
        assert_eq!(fill("{{#each rows}}x{{/each}}", &Args::new()).unwrap(), "");
        let mut wrong = Args::new();
        wrong.insert("rows".to_string(), Arg::from("not-a-list"));
        assert_eq!(
            fill("{{#each rows}}x{{/each}}", &wrong).unwrap_err(),
            TemplateError::EachArgType {
                key: "rows".to_string(),
                type_name: "string".to_string()
            }
        );
        let mut scalar_items = Args::new();
        scalar_items.insert("rows".to_string(), Arg::List(vec![Arg::from("x")]));
        assert_eq!(
            fill("{{#each rows}}x{{/each}}", &scalar_items).unwrap_err(),
            TemplateError::EachItemType {
                key: "rows".to_string(),
                index: 0,
                type_name: "string".to_string()
            }
        );
    }

    #[test]
    fn each_scope_shadows_outer_keys_and_renders_inline() {
        let mut row = BTreeMap::new();
        row.insert("label".to_string(), Arg::from("A"));
        row.insert("command".to_string(), Arg::from("/a 1"));
        let mut source = Args::new();
        source.insert("label".to_string(), Arg::from("外层"));
        source.insert("rows".to_string(), Arg::List(vec![Arg::Map(row)]));
        let rendered = fill(
            "{{#each rows}}{{label}}={{inline label command}}{{/each}}",
            &source,
        )
        .unwrap();
        assert_eq!(
            rendered,
            "A=<qqbot-cmd-input text=\"%2Fa%201\" show=\"A\" />"
        );
    }
}
