use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginConfigKind {
    #[default]
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "textarea")]
    Textarea,
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "boolean")]
    Boolean,
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "password")]
    Password,
}

impl PluginConfigKind {
    pub const ALL: [Self; 6] = [
        Self::Text,
        Self::Textarea,
        Self::Number,
        Self::Boolean,
        Self::Select,
        Self::Password,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Textarea => "textarea",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Select => "select",
            Self::Password => "password",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Text => "文本",
            Self::Textarea => "多行文本",
            Self::Number => "数字",
            Self::Boolean => "开关",
            Self::Select => "单选项",
            Self::Password => "密钥",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let lowered = raw.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == lowered)
            .or(match lowered.as_str() {
                "bool" | "switch" | "checkbox" => Some(Self::Boolean),
                "int" | "integer" | "float" => Some(Self::Number),
                "choice" | "enum" => Some(Self::Select),
                "secret" => Some(Self::Password),
                "string" => Some(Self::Text),
                _ => None,
            })
    }

    pub fn coerce(&self, value: &Value) -> Option<Value> {
        match self {
            Self::Text | Self::Textarea | Self::Select | Self::Password => match value {
                Value::String(text) => Some(Value::String(text.clone())),
                Value::Number(_) | Value::Bool(_) => Some(Value::String(value.to_string())),
                _ => None,
            },
            Self::Number => match value {
                Value::Number(_) => Some(value.clone()),
                Value::String(text) => {
                    let trimmed = text.trim();
                    if let Ok(integer) = trimmed.parse::<i64>() {
                        return Some(Value::from(integer));
                    }
                    trimmed
                        .parse::<f64>()
                        .ok()
                        .and_then(Number::from_f64)
                        .map(Value::Number)
                }
                _ => None,
            },
            Self::Boolean => match value {
                Value::Bool(_) => Some(value.clone()),
                Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "on" | "yes" => Some(Value::Bool(true)),
                    "false" | "0" | "off" | "no" => Some(Value::Bool(false)),
                    _ => None,
                },
                Value::Number(number) => match number.as_i64() {
                    Some(1) => Some(Value::Bool(true)),
                    Some(0) => Some(Value::Bool(false)),
                    _ => None,
                },
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginConfigField {
    pub key: String,
    pub label: String,
    #[serde(default, rename = "type")]
    pub kind: PluginConfigKind,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl PluginConfigField {
    fn new(key: impl Into<String>, label: impl Into<String>, kind: PluginConfigKind) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            kind,
            default: Value::Null,
            description: String::new(),
            placeholder: String::new(),
            options: Vec::new(),
            required: false,
        }
    }

    pub fn text(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, PluginConfigKind::Text)
    }

    pub fn textarea(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, PluginConfigKind::Textarea)
    }

    pub fn number(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, PluginConfigKind::Number)
    }

    pub fn boolean(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, PluginConfigKind::Boolean)
    }

    pub fn password(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, PluginConfigKind::Password)
    }

    pub fn select(
        key: impl Into<String>,
        label: impl Into<String>,
        options: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let mut field = Self::new(key, label, PluginConfigKind::Select);
        field.options = options.into_iter().map(Into::into).collect();
        field
    }

    pub fn default_value(mut self, value: impl Into<Value>) -> Self {
        self.default = value.into();
        self
    }

    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = text.into();
        self
    }

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn check(&self, value: &Value) -> Result<Value, String> {
        let coerced = self.kind.coerce(value).ok_or_else(|| {
            format!(
                "配置项「{}」需要{}值, 当前收到 {value}",
                self.label,
                self.kind.label()
            )
        })?;
        if self.kind == PluginConfigKind::Select && !self.options.is_empty() {
            let text = coerced.as_str().unwrap_or_default();
            if !self.options.iter().any(|option| option == text) {
                return Err(format!(
                    "配置项「{}」只能填 {} 之一, 当前收到 `{text}`",
                    self.label,
                    self.options.join("、")
                ));
            }
        }
        Ok(coerced)
    }
}

pub fn find_field<'a>(fields: &'a [PluginConfigField], key: &str) -> Option<&'a PluginConfigField> {
    fields.iter().find(|field| field.key == key)
}

pub fn validate_values(
    fields: &[PluginConfigField],
    values: &BTreeMap<String, Value>,
) -> Result<(), String> {
    for (key, value) in values {
        let Some(field) = find_field(fields, key) else {
            return Err(format!("插件没有声明配置项「{key}」"));
        };
        field.check(value)?;
    }
    Ok(())
}

pub fn normalize_values(
    fields: &[PluginConfigField],
    values: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, String> {
    let mut normalized = BTreeMap::new();
    for (key, value) in values {
        let Some(field) = find_field(fields, key) else {
            return Err(format!("插件没有声明配置项「{key}」"));
        };
        normalized.insert(key.clone(), field.check(value)?);
    }
    Ok(normalized)
}

pub fn defaults(fields: &[PluginConfigField]) -> BTreeMap<String, Value> {
    let mut values = BTreeMap::new();
    for field in fields {
        if !field.default.is_null() {
            values.insert(field.key.clone(), field.default.clone());
        }
    }
    values
}

pub fn resumed_values(
    fields: &[PluginConfigField],
    stored: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut values = defaults(fields);
    for (key, value) in stored {
        values.insert(key.clone(), value.clone());
    }
    values
}

pub fn fields_json(fields: &[PluginConfigField]) -> Vec<Value> {
    fields
        .iter()
        .filter_map(|field| serde_json::to_value(field).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Vec<PluginConfigField> {
        vec![
            PluginConfigField::text("api_key", "接口密钥")
                .description("开放平台的接口密钥")
                .required(),
            PluginConfigField::number("limit", "次数上限").default_value(10),
            PluginConfigField::boolean("enabled", "启用").default_value(true),
            PluginConfigField::select("mode", "模式", ["宽松", "严格"]).default_value("严格"),
        ]
    }

    #[test]
    fn kind_names_match_the_admin_drawer() {
        assert_eq!(PluginConfigKind::Text.as_str(), "text");
        assert_eq!(PluginConfigKind::Textarea.as_str(), "textarea");
        assert_eq!(PluginConfigKind::Number.as_str(), "number");
        assert_eq!(PluginConfigKind::Boolean.as_str(), "boolean");
        assert_eq!(PluginConfigKind::Select.as_str(), "select");
        assert_eq!(PluginConfigKind::Password.as_str(), "password");
        assert_eq!(
            PluginConfigKind::parse("bool"),
            Some(PluginConfigKind::Boolean)
        );
        assert_eq!(
            PluginConfigKind::parse("SWITCH"),
            Some(PluginConfigKind::Boolean)
        );
        assert_eq!(PluginConfigKind::parse("枚举"), None);
    }

    #[test]
    fn field_json_uses_the_type_key_the_drawer_reads() {
        let fields = sample();
        let encoded = fields_json(&fields);
        assert_eq!(encoded.len(), 4, "四个配置项都要导出");
        assert_eq!(encoded[0]["key"], json!("api_key"));
        assert_eq!(encoded[0]["label"], json!("接口密钥"));
        assert_eq!(encoded[0]["type"], json!("text"));
        assert_eq!(encoded[0]["required"], json!(true));
        assert_eq!(encoded[0]["description"], json!("开放平台的接口密钥"));
        assert_eq!(encoded[3]["type"], json!("select"));
        assert_eq!(encoded[3]["options"], json!(["宽松", "严格"]));
    }

    #[test]
    fn defaults_are_reported_as_current_values() {
        let fields = sample();
        let values = defaults(&fields);
        assert_eq!(values.get("limit"), Some(&json!(10)));
        assert_eq!(values.get("enabled"), Some(&json!(true)));
        assert_eq!(values.get("mode"), Some(&json!("严格")));
        assert!(!values.contains_key("api_key"), "空默认值不写入当前值");
    }

    #[test]
    fn stored_values_win_over_defaults() {
        let fields = sample();
        let mut stored = BTreeMap::new();
        stored.insert(String::from("limit"), json!(3));
        let values = resumed_values(&fields, &stored);
        assert_eq!(values.get("limit"), Some(&json!(3)));
        assert_eq!(
            values.get("mode"),
            Some(&json!("严格")),
            "没存的项回落到默认值"
        );
    }

    #[test]
    fn unknown_key_is_rejected() {
        let fields = sample();
        let mut values = BTreeMap::new();
        values.insert(String::from("nope"), json!(1));
        let error = validate_values(&fields, &values).expect_err("未声明的键必须被拒绝");
        assert!(error.contains("「nope」"), "错误里要点出键名: {error}");
    }

    #[test]
    fn number_and_boolean_accept_string_forms() {
        let fields = sample();
        let mut values = BTreeMap::new();
        values.insert(String::from("limit"), json!("25"));
        values.insert(String::from("enabled"), json!("off"));
        let normalized = normalize_values(&fields, &values).expect("字符串形式要能归一");
        assert_eq!(normalized.get("limit"), Some(&json!(25)));
        assert_eq!(normalized.get("enabled"), Some(&json!(false)));
    }

    #[test]
    fn number_rejects_text() {
        let fields = sample();
        let mut values = BTreeMap::new();
        values.insert(String::from("limit"), json!("很多"));
        let error = validate_values(&fields, &values).expect_err("文本不能填进数字项");
        assert!(error.contains("需要数字值"), "错误要说清类型要求: {error}");
    }

    #[test]
    fn select_rejects_value_outside_options() {
        let fields = sample();
        let mut values = BTreeMap::new();
        values.insert(String::from("mode"), json!("中间"));
        let error = validate_values(&fields, &values).expect_err("不在选项内必须被拒绝");
        assert!(error.contains("宽松、严格"), "错误要列出可选值: {error}");
    }

    #[test]
    fn valid_values_pass_validation() {
        let fields = sample();
        let mut values = BTreeMap::new();
        values.insert(String::from("api_key"), json!("abc"));
        values.insert(String::from("limit"), json!(5));
        values.insert(String::from("enabled"), json!(true));
        values.insert(String::from("mode"), json!("宽松"));
        validate_values(&fields, &values).expect("合法配置要能通过");
    }
}
