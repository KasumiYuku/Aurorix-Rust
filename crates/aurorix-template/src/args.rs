use std::collections::BTreeMap;

use crate::error::{TemplateError, TemplateResult};

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Str(String),
    Bool(bool),
    Int(i64),
    Float(f64),
    List(Vec<Arg>),
    Map(BTreeMap<String, Arg>),
    Null,
    Unsupported(&'static str),
}

impl Arg {
    pub fn type_label(&self) -> &'static str {
        match self {
            Arg::Str(_) => "string",
            Arg::Bool(_) => "bool",
            Arg::Int(_) => "int64",
            Arg::Float(_) => "float64",
            Arg::List(_) => "[]any",
            Arg::Map(_) => "map[string]any",
            Arg::Null => "<nil>",
            Arg::Unsupported(name) => name,
        }
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Arg::Str(value.to_string())
    }
}

impl From<String> for Arg {
    fn from(value: String) -> Self {
        Arg::Str(value)
    }
}

impl From<bool> for Arg {
    fn from(value: bool) -> Self {
        Arg::Bool(value)
    }
}

impl From<i64> for Arg {
    fn from(value: i64) -> Self {
        Arg::Int(value)
    }
}

impl From<i32> for Arg {
    fn from(value: i32) -> Self {
        Arg::Int(i64::from(value))
    }
}

impl From<usize> for Arg {
    fn from(value: usize) -> Self {
        Arg::Int(value as i64)
    }
}

impl From<f64> for Arg {
    fn from(value: f64) -> Self {
        Arg::Float(value)
    }
}

impl From<Vec<Arg>> for Arg {
    fn from(value: Vec<Arg>) -> Self {
        Arg::List(value)
    }
}

impl From<BTreeMap<String, Arg>> for Arg {
    fn from(value: BTreeMap<String, Arg>) -> Self {
        Arg::Map(value)
    }
}

impl From<serde_json::Value> for Arg {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Arg::Null,
            serde_json::Value::Bool(flag) => Arg::Bool(flag),
            serde_json::Value::Number(number) => match number.as_i64() {
                Some(int) => Arg::Int(int),
                None => Arg::Float(number.as_f64().unwrap_or(f64::NAN)),
            },
            serde_json::Value::String(text) => Arg::Str(text),
            serde_json::Value::Array(items) => {
                Arg::List(items.into_iter().map(Arg::from).collect())
            }
            serde_json::Value::Object(map) => Arg::Map(
                map.into_iter()
                    .map(|(key, item)| (key, Arg::from(item)))
                    .collect(),
            ),
        }
    }
}

pub type Args = BTreeMap<String, Arg>;

pub type FlatArgs = BTreeMap<String, String>;

pub fn arg_map<const N: usize>(pairs: [(&str, Arg); N]) -> Args {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[macro_export]
macro_rules! args_map {
    ($($key:literal => $value:expr),* $(,)?) => {{
        let mut map = ::std::collections::BTreeMap::new();
        $(map.insert($key.to_string(), $crate::Arg::from($value));)*
        map
    }};
}

pub fn format_float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "Inf"
        } else {
            "-Inf"
        }
        .to_string();
    }
    let rendered = format!("{value}");
    if rendered.contains('e') || rendered.contains('E') {
        expand_exponent(&rendered)
    } else {
        rendered
    }
}

fn expand_exponent(rendered: &str) -> String {
    let Some((mantissa, exponent)) = rendered.split_once(['e', 'E']) else {
        return rendered.to_string();
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return rendered.to_string();
    };
    expand_decimal(mantissa, exponent)
}

pub fn expand_decimal(mantissa: &str, exponent: i32) -> String {
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => (int_part.to_string(), frac_part.to_string()),
        None => (mantissa.to_string(), String::new()),
    };
    let digits = format!("{int_part}{frac_part}");
    let point = int_part.len() as i32 + exponent;
    let mut out = String::from(sign);
    if point <= 0 {
        out.push_str("0.");
        for _ in 0..(-point) {
            out.push('0');
        }
        out.push_str(&digits);
        return out;
    }
    let point = point as usize;
    if point >= digits.len() {
        out.push_str(&digits);
        for _ in 0..(point - digits.len()) {
            out.push('0');
        }
        return out;
    }
    out.push_str(&digits[..point]);
    out.push('.');
    out.push_str(&digits[point..]);
    out
}

pub fn flatten(source: &Args) -> TemplateResult<FlatArgs> {
    let mut flat = FlatArgs::new();
    for (key, value) in source {
        walk(key, value, &mut flat)?;
    }
    Ok(flat)
}

fn walk(prefix: &str, value: &Arg, flat: &mut FlatArgs) -> TemplateResult<()> {
    match value {
        Arg::Str(text) => {
            flat.insert(prefix.to_string(), text.clone());
        }
        Arg::Bool(flag) => {
            flat.insert(prefix.to_string(), flag.to_string());
        }
        Arg::Int(int) => {
            flat.insert(prefix.to_string(), int.to_string());
        }
        Arg::Float(float) => {
            flat.insert(prefix.to_string(), format_float(*float));
        }
        Arg::Map(map) => {
            for (child_key, child) in map {
                walk(&format!("{prefix}.{child_key}"), child, flat)?;
            }
        }
        Arg::List(items) => {
            for (index, child) in items.iter().enumerate() {
                walk(&format!("{prefix}.#{index}"), child, flat)?;
            }
        }
        Arg::Null => {}
        Arg::Unsupported(name) => {
            return Err(TemplateError::UnsupportedArgType {
                key: prefix.to_string(),
                type_name: (*name).to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_matches_go_nested_layout() {
        let mut user = BTreeMap::new();
        user.insert("name".to_string(), Arg::from("x"));
        user.insert("n".to_string(), Arg::from(1i64));
        let mut source = Args::new();
        source.insert("title".to_string(), Arg::from("标题"));
        source.insert("count".to_string(), Arg::from(3i64));
        source.insert(
            "list".to_string(),
            Arg::List(vec![Arg::from("a"), Arg::from("b")]),
        );
        source.insert("obj".to_string(), Arg::Map(user));
        let flat = flatten(&source).unwrap();
        assert_eq!(flat["title"], "标题");
        assert_eq!(flat["count"], "3");
        assert_eq!(flat["list.#0"], "a");
        assert_eq!(flat["list.#1"], "b");
        assert_eq!(flat["obj.name"], "x");
        assert_eq!(flat["obj.n"], "1");
    }

    #[test]
    fn flatten_rejects_unsupported_type() {
        let mut source = Args::new();
        source.insert("bad".to_string(), Arg::Unsupported("chan int"));
        let err = flatten(&source).unwrap_err();
        assert_eq!(
            err,
            TemplateError::UnsupportedArgType {
                key: "bad".to_string(),
                type_name: "chan int".to_string(),
            }
        );
        assert_eq!(err.to_string(), "key bad has unsupported type: chan int");
    }

    #[test]
    fn null_is_dropped_bool_and_float_use_go_spelling() {
        let mut source = Args::new();
        source.insert("nothing".to_string(), Arg::Null);
        source.insert("yes".to_string(), Arg::from(true));
        source.insert("ratio".to_string(), Arg::from(0.5f64));
        source.insert("whole".to_string(), Arg::from(3.0f64));
        let flat = flatten(&source).unwrap();
        assert!(!flat.contains_key("nothing"));
        assert_eq!(flat["yes"], "true");
        assert_eq!(flat["ratio"], "0.5");
        assert_eq!(flat["whole"], "3");
    }

    #[test]
    fn float_without_exponent_never_uses_exponent_form() {
        assert_eq!(format_float(1e21), "1000000000000000000000");
        assert_eq!(format_float(1e-7), "0.0000001");
        assert_eq!(format_float(-2.5e-3), "-0.0025");
        assert_eq!(format_float(f64::INFINITY), "Inf");
        assert_eq!(format_float(f64::NEG_INFINITY), "-Inf");
        assert_eq!(format_float(f64::NAN), "NaN");
    }

    #[test]
    fn decimal_expansion_hits_every_boundary() {
        assert_eq!(expand_decimal("1", 0), "1");
        assert_eq!(expand_decimal("1", -1), "0.1");
        assert_eq!(expand_decimal("15", -2), "0.15");
        assert_eq!(expand_decimal("1", -5), "0.00001");
        assert_eq!(expand_decimal("1", 2), "100");
        assert_eq!(expand_decimal("1", 21), "1000000000000000000000");
        assert_eq!(expand_decimal("123", -1), "12.3");
        assert_eq!(expand_decimal("-25", -2), "-0.25");
        assert_eq!(expand_decimal("1500", -3), "1.500");
    }

    #[test]
    fn serde_json_numbers_become_int_or_float() {
        assert_eq!(Arg::from(serde_json::json!(7)), Arg::Int(7));
        assert_eq!(Arg::from(serde_json::json!(1.5)), Arg::Float(1.5));
        assert_eq!(Arg::from(serde_json::json!(null)), Arg::Null);
        assert_eq!(Arg::from(serde_json::json!("x")), Arg::Str("x".into()));
    }
}
