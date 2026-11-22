use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    Value,
    Switch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagSpec {
    pub name: String,
    pub short: Option<char>,
    pub kind: ArgKind,
    pub help: String,
    pub default: Option<String>,
}

impl FlagSpec {
    pub fn value(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            short: None,
            kind: ArgKind::Value,
            help: String::new(),
            default: None,
        }
    }

    pub fn switch(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            short: None,
            kind: ArgKind::Switch,
            help: String::new(),
            default: None,
        }
    }

    pub fn short(mut self, short: char) -> Self {
        self.short = Some(short);
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        self.default = Some(value.into());
        self
    }

    pub fn summary(&self) -> String {
        let head = match self.short {
            Some(short) => format!("-{short}, --{}", self.name),
            None => format!("    --{}", self.name),
        };
        match self.kind {
            ArgKind::Value => format!("{head}=STRING"),
            ArgKind::Switch => head,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionalSpec {
    pub name: String,
    pub required: bool,
    pub variadic: bool,
    pub help: String,
}

impl PositionalSpec {
    pub fn required(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            required: true,
            variadic: false,
            help: String::new(),
        }
    }

    pub fn optional(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            required: false,
            variadic: false,
            help: String::new(),
        }
    }

    pub fn variadic(mut self) -> Self {
        self.variadic = true;
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    pub fn summary(&self) -> String {
        match (self.variadic, self.required) {
            (true, _) => format!("[<{}> ...]", self.name),
            (false, true) => format!("<{}>", self.name),
            (false, false) => format!("[<{}>]", self.name),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArgsSpec {
    pub positionals: Vec<PositionalSpec>,
    pub flags: Vec<FlagSpec>,
}

impl ArgsSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn positional(mut self, spec: PositionalSpec) -> Self {
        self.positionals.push(spec);
        self
    }

    pub fn flag(mut self, flag: FlagSpec) -> Self {
        self.flags.push(flag);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.positionals.is_empty() && self.flags.is_empty()
    }

    pub fn usage(&self, name: &str) -> String {
        let mut head = format!("用法: {name}");
        for positional in &self.positionals {
            head.push(' ');
            head.push_str(&positional.summary());
        }
        if !self.flags.is_empty() {
            head.push_str(" [选项]");
        }
        let mut blocks = vec![head];
        if !self.positionals.is_empty() {
            let mut block = String::from("参数:");
            for positional in &self.positionals {
                block.push_str(&detail_line(&positional.summary(), &positional.help));
            }
            blocks.push(block);
        }
        if !self.flags.is_empty() {
            let mut block = String::from("选项:");
            for flag in &self.flags {
                block.push_str(&detail_line(&flag.summary(), &flag.help));
            }
            blocks.push(block);
        }
        blocks.join("\n\n")
    }

    pub fn find_long(&self, name: &str) -> Option<&FlagSpec> {
        self.flags.iter().find(|flag| flag.name == name)
    }

    pub fn find_short(&self, short: char) -> Option<&FlagSpec> {
        self.flags.iter().find(|flag| flag.short == Some(short))
    }
}

fn detail_line(summary: &str, help: &str) -> String {
    if help.is_empty() {
        format!("\n  {summary}")
    } else {
        format!("\n  {summary}    {help}")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedArgs {
    pub positional: Vec<String>,
    pub named: BTreeMap<String, String>,
    pub flags: BTreeMap<String, String>,
    pub switches: BTreeSet<String>,
}

impl ParsedArgs {
    pub fn from_tokens(tokens: &[String]) -> Self {
        Self {
            positional: tokens.to_vec(),
            named: BTreeMap::new(),
            flags: BTreeMap::new(),
            switches: BTreeSet::new(),
        }
    }

    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }

    pub fn switch(&self, name: &str) -> bool {
        self.switches.contains(name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.named
            .get(name)
            .or_else(|| self.flags.get(name))
            .map(String::as_str)
    }

    pub fn positional_value(&self, name: &str) -> Option<&str> {
        self.named.get(name).map(String::as_str)
    }

    pub fn first(&self) -> Option<&str> {
        self.positional.first().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.positional.is_empty()
            && self.flags.is_empty()
            && self.switches.is_empty()
            && self.named.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgError {
    pub message: String,
    pub usage: String,
}

impl ArgError {
    pub fn new(message: impl Into<String>, usage: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            usage: usage.into(),
        }
    }
}

impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ArgError {}

pub fn tokenize(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars();
    while let Some(current_char) = chars.next() {
        match quote {
            Some(active) => {
                if current_char == '\\' {
                    match chars.next() {
                        Some(escaped) => current.push(escaped),
                        None => return Err("反斜杠后缺少字符".to_string()),
                    }
                } else if current_char == active {
                    quote = None;
                } else {
                    current.push(current_char);
                }
            }
            None => {
                if current_char == '"' || current_char == '\'' {
                    quote = Some(current_char);
                    started = true;
                } else if current_char == '\\' {
                    match chars.next() {
                        Some(escaped) => {
                            current.push(escaped);
                            started = true;
                        }
                        None => return Err("反斜杠后缺少字符".to_string()),
                    }
                } else if current_char.is_whitespace() {
                    if started || !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                        started = false;
                    }
                } else {
                    current.push(current_char);
                    started = true;
                }
            }
        }
    }
    if quote.is_some() {
        return Err("引号没有闭合".to_string());
    }
    if started || !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

pub fn parse_args(name: &str, spec: &ArgsSpec, input: &str) -> Result<ParsedArgs, ArgError> {
    let usage = spec.usage(name);
    let tokens = tokenize(input).map_err(|message| ArgError::new(message, usage.clone()))?;
    parse_tokens(name, spec, &tokens)
}

pub fn parse_tokens(
    name: &str,
    spec: &ArgsSpec,
    tokens: &[String],
) -> Result<ParsedArgs, ArgError> {
    let usage = spec.usage(name);
    let mut parsed = ParsedArgs::default();
    for flag in &spec.flags {
        if let Some(default) = &flag.default {
            parsed.flags.insert(flag.name.clone(), default.clone());
        }
    }
    let mut collected: Vec<String> = Vec::new();
    let mut index = 0usize;
    let mut literal = false;
    while index < tokens.len() {
        let token = tokens[index].as_str();
        index += 1;
        if literal {
            collected.push(token.to_string());
            continue;
        }
        if token == "--" {
            literal = true;
            continue;
        }
        if let Some(body) = token.strip_prefix("--") {
            let (flag_name, inline) = match body.split_once('=') {
                Some((head, value)) => (head, Some(value.to_string())),
                None => (body, None),
            };
            let Some(flag) = spec.find_long(flag_name) else {
                collected.push(token.to_string());
                continue;
            };
            match flag.kind {
                ArgKind::Switch => {
                    if let Some(value) = inline {
                        return Err(ArgError::new(
                            format!("unexpected flag argument {value:?}"),
                            usage,
                        ));
                    }
                    parsed.switches.insert(flag.name.clone());
                }
                ArgKind::Value => {
                    let value = match inline {
                        Some(value) => value,
                        None => match tokens.get(index) {
                            Some(next) => {
                                index += 1;
                                next.clone()
                            }
                            None => {
                                return Err(ArgError::new(format!("--{} 缺少值", flag.name), usage))
                            }
                        },
                    };
                    parsed.flags.insert(flag.name.clone(), value);
                }
            }
            continue;
        }
        if token.len() > 1 && token.starts_with('-') {
            let mut chars = token[1..].chars();
            let head = chars.next().unwrap_or('-');
            let tail: String = chars.collect();
            let Some(flag) = spec.find_short(head) else {
                collected.push(token.to_string());
                continue;
            };
            match flag.kind {
                ArgKind::Switch => {
                    if !tail.is_empty() {
                        return Err(ArgError::new(
                            format!("unexpected flag argument {tail:?}"),
                            usage,
                        ));
                    }
                    parsed.switches.insert(flag.name.clone());
                }
                ArgKind::Value => {
                    let value = if tail.is_empty() {
                        match tokens.get(index) {
                            Some(next) => {
                                index += 1;
                                next.clone()
                            }
                            None => {
                                return Err(ArgError::new(format!("-{} 缺少值", flag.name), usage))
                            }
                        }
                    } else {
                        tail
                    };
                    parsed.flags.insert(flag.name.clone(), value);
                }
            }
            continue;
        }
        collected.push(token.to_string());
    }
    parsed.positional = assign_positionals(spec, &collected, &usage)?;
    for (index, positional) in spec.positionals.iter().enumerate() {
        if let Some(value) = parsed.positional.get(index) {
            if !value.is_empty() {
                parsed.named.insert(positional.name.clone(), value.clone());
            }
        }
    }
    Ok(parsed)
}

fn assign_positionals(
    spec: &ArgsSpec,
    collected: &[String],
    usage: &str,
) -> Result<Vec<String>, ArgError> {
    let variadic = spec
        .positionals
        .last()
        .map(|positional| positional.variadic)
        .unwrap_or(false);
    let allowed = if variadic {
        collected.len()
    } else {
        spec.positionals.len()
    };
    if collected.len() > allowed {
        let extra = &collected[allowed];
        return Err(ArgError::new(
            format!("unexpected argument {extra}"),
            usage.to_string(),
        ));
    }
    let mut missing = Vec::new();
    for (index, positional) in spec.positionals.iter().enumerate() {
        if positional.required && index >= collected.len() {
            missing.push(format!("<{}>", positional.name));
        }
    }
    if !missing.is_empty() {
        return Err(ArgError::new(
            format!("missing positional arguments {}", missing.join(" ")),
            usage.to_string(),
        ));
    }
    Ok(collected.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ArgsSpec {
        ArgsSpec::new()
            .positional(PositionalSpec::required("name").help("歌名"))
            .positional(PositionalSpec::optional("index").variadic().help("下标"))
            .flag(FlagSpec::value("source").short('s').help("音源"))
            .flag(FlagSpec::switch("raw").short('r').help("原文"))
    }

    #[test]
    fn positional_only() {
        let parsed = parse_args("p song", &spec(), "Lyrith").expect("缺省下标应当可选");
        assert_eq!(parsed.positional, vec!["Lyrith".to_string()]);
        assert!(parsed.flags.is_empty(), "不该凭空冒出 flag");
        assert!(parsed.switches.is_empty(), "不该凭空冒出开关");
    }

    #[test]
    fn variadic_keeps_every_word() {
        let parsed = parse_args("p song", &spec(), "Lyrith 0 1 2").expect("可变参数应当收下全部词");
        assert_eq!(
            parsed.positional,
            vec![
                "Lyrith".to_string(),
                "0".to_string(),
                "1".to_string(),
                "2".to_string()
            ]
        );
    }

    #[test]
    fn long_flag_and_short_flag() {
        let parsed = parse_args("p song", &spec(), "花海 -s wyy").expect("短选项应当生效");
        assert_eq!(parsed.flag("source"), Some("wyy"));
        let parsed = parse_args("p song", &spec(), "--source wyy 花海").expect("长选项应当生效");
        assert_eq!(parsed.flag("source"), Some("wyy"));
        assert_eq!(parsed.first(), Some("花海"));
    }

    #[test]
    fn inline_value_with_equals() {
        let parsed = parse_args("p song", &spec(), "--source=wyy 花海").expect("等号形式应当生效");
        assert_eq!(parsed.flag("source"), Some("wyy"));
        let parsed = parse_args("p song", &spec(), "-swyy 花海").expect("短选项贴值应当生效");
        assert_eq!(parsed.flag("source"), Some("wyy"));
    }

    #[test]
    fn switch_with_dash_and_double_dash() {
        let parsed = parse_args("p song", &spec(), "-r 花海").expect("开关应当生效");
        assert!(parsed.switch("raw"));
        let parsed = parse_args("p song", &spec(), "--raw 花海").expect("长开关应当生效");
        assert!(parsed.switch("raw"));
    }

    #[test]
    fn quotes_are_stripped() {
        let parsed =
            parse_args("echo", &spec(), "\"花 海\" 'wyy x'").expect("引号包裹应当成一个词");
        assert_eq!(
            parsed.positional,
            vec!["花 海".to_string(), "wyy x".to_string()]
        );
    }

    #[test]
    fn escapes_are_resolved() {
        let parsed = parse_args("echo", &spec(), "a\\ b \"c\\\"d\"").expect("转义应当生效");
        assert_eq!(
            parsed.positional,
            vec!["a b".to_string(), "c\"d".to_string()]
        );
    }

    #[test]
    fn unterminated_quote_is_rejected() {
        let err = parse_args("echo", &spec(), "\"花海").unwrap_err();
        assert_eq!(err.message, "引号没有闭合");
        assert!(err.usage.contains("用法: echo"), "错误要带用法");
    }

    #[test]
    fn missing_positional_text_matches_go() {
        let err = parse_args("p song", &spec(), "").unwrap_err();
        assert_eq!(err.message, "missing positional arguments <name>");
    }

    #[test]
    fn extra_positional_text_matches_go() {
        let tight = ArgsSpec::new().positional(PositionalSpec::required("name"));
        let err = parse_args("echo", &tight, "a b").unwrap_err();
        assert_eq!(err.message, "unexpected argument b");
    }

    #[test]
    fn dash_value_stays_positional() {
        let parsed =
            parse_args("p song", &spec(), "Lyrith -迷宮リリス-").expect("带横线的值不该报错");
        assert_eq!(parsed.positional[1], "-迷宮リリス-");
        assert!(parsed.switches.is_empty(), "未知开关不该被收下");
    }

    #[test]
    fn unknown_flag_becomes_positional() {
        let loose = ArgsSpec::new().positional(PositionalSpec::optional("word").variadic());
        let parsed =
            parse_args("echo", &loose, "--nope 1").expect("未知开关按 Go 的做法当位置参数");
        assert_eq!(
            parsed.positional,
            vec!["--nope".to_string(), "1".to_string()]
        );
    }

    #[test]
    fn double_dash_ends_flags() {
        let parsed = parse_args("echo", &spec(), "-- -s").expect("双横线之后全是位置参数");
        assert_eq!(parsed.positional, vec!["-s".to_string()]);
    }

    #[test]
    fn switch_rejects_inline_value() {
        let err = parse_args("p song", &spec(), "--raw=1 花海").unwrap_err();
        assert_eq!(err.message, "unexpected flag argument \"1\"");
        let err = parse_args("p song", &spec(), "-r1 花海").unwrap_err();
        assert_eq!(err.message, "unexpected flag argument \"1\"");
    }

    #[test]
    fn value_flag_without_value_is_rejected() {
        let err = parse_args("p song", &spec(), "花海 --source").unwrap_err();
        assert_eq!(err.message, "--source 缺少值");
        let err = parse_args("p song", &spec(), "-s").unwrap_err();
        assert_eq!(err.message, "-source 缺少值");
    }

    #[test]
    fn defaults_are_filled() {
        let with_default = ArgsSpec::new().flag(FlagSpec::value("source").default_value("netease"));
        let parsed = parse_args("p song", &with_default, "").expect("空输入应当用默认值");
        assert_eq!(parsed.flag("source"), Some("netease"));
    }

    #[test]
    fn usage_document_keeps_localized_labels() {
        let text = spec().usage("p song");
        assert!(text.starts_with("用法: p song <name> [<index> ...] [选项]"));
        assert!(text.contains("参数:"), "用法要带参数段");
        assert!(text.contains("选项:"), "用法要带选项段");
        assert!(text.contains("-s, --source=STRING"), "用法要带选项签名");
        assert!(text.contains("音源"), "用法要带选项说明");
    }

    #[test]
    fn args_spec_is_empty_tracks_both_lists() {
        assert!(ArgsSpec::new().is_empty(), "空规格应当算空");
        assert!(
            !ArgsSpec::new()
                .positional(PositionalSpec::required("a"))
                .is_empty(),
            "只有位置参数不算空"
        );
        assert!(
            !ArgsSpec::new().flag(FlagSpec::switch("raw")).is_empty(),
            "只有选项不算空"
        );
    }

    #[test]
    fn tokens_helper_reads_positional_only() {
        let parsed = ParsedArgs::from_tokens(&["a".to_string(), "b".to_string()]);
        assert_eq!(parsed.positional.len(), 2);
        assert!(parsed.value("x").is_none());
        assert!(!parsed.is_empty());
        assert!(ParsedArgs::default().is_empty());
    }
}
