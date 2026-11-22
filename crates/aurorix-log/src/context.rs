use std::sync::OnceLock;

use regex::Regex;

pub const CONTEXT_PATTERN: &str = r"\b(group|user)=([^\s,;]+)";

pub fn extract_context(msg: &str) -> (String, String) {
    let mut group = String::new();
    let mut user = String::new();
    if !msg.contains("group=") && !msg.contains("user=") {
        return (group, user);
    }
    for found in pattern().captures_iter(msg) {
        let key = found.get(1).map(|item| item.as_str()).unwrap_or("");
        let value = found.get(2).map(|item| item.as_str()).unwrap_or("");
        match key {
            "group" if group.is_empty() => group = value.to_string(),
            "user" if user.is_empty() => user = value.to_string(),
            _ => {}
        }
    }
    (group, user)
}

fn pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(CONTEXT_PATTERN).expect("上下文正则"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degraded_group_message_carries_both_fields() {
        let (group, user) = extract_context(
            "被动回复窗口已失效 target=0 group=819EFED8D4D90696B758AE620B59546C user=618F87D017231E57C5B8DCDD07761AAB err=x",
        );
        assert_eq!(group, "819EFED8D4D90696B758AE620B59546C");
        assert_eq!(user, "618F87D017231E57C5B8DCDD07761AAB");
    }

    #[test]
    fn comma_separated_fields_stop_at_the_separator() {
        let (group, user) = extract_context("同步失败 group=G1, user=U1, 其它");
        assert_eq!(group, "G1");
        assert_eq!(user, "U1");
    }

    #[test]
    fn user_only_message_has_no_group() {
        let (group, user) = extract_context("只带 user=U2");
        assert_eq!(group, "");
        assert_eq!(user, "U2");
    }

    #[test]
    fn message_without_context_yields_empty_fields() {
        let (group, user) = extract_context("图床转换成功 provider=x");
        assert_eq!(group, "");
        assert_eq!(user, "");
    }

    #[test]
    fn lookalike_keys_are_not_matched() {
        let (group, user) = extract_context("groups=1 users=2");
        assert_eq!(group, "");
        assert_eq!(user, "");
    }

    #[test]
    fn first_occurrence_wins() {
        let (group, user) = extract_context("group=G1 group=G2 user=U1 user=U2");
        assert_eq!(group, "G1");
        assert_eq!(user, "U1");
    }

    #[test]
    fn uppercase_keys_are_not_matched() {
        let (group, user) = extract_context("Group=G1 User=U1");
        assert_eq!(group, "");
        assert_eq!(user, "");
    }
}
