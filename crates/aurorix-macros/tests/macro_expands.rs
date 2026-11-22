use aurorix_macros::command;

#[command("/ping")]
fn ping() -> &'static str {
    "pong"
}

#[command("/ban", admin)]
pub fn ban(user: &str) -> String {
    format!("已封禁 {user}")
}

#[command("/b50")]
fn b50_of(user: &str, limit: u32) -> String {
    format!("{user} 的前 {limit} 条成绩")
}

#[test]
fn constant_carries_command_name_and_admin_flag() {
    assert_eq!(__AURORIX_CMD_PING, ("/ping", false));
    assert_eq!(__AURORIX_CMD_BAN, ("/ban", true));
    assert_eq!(__AURORIX_CMD_B50_OF, ("/b50", false));
}

#[test]
fn function_body_is_left_untouched() {
    assert_eq!(ping(), "pong");
    assert_eq!(ban("u1"), "已封禁 u1");
    assert_eq!(b50_of("u2", 10), "u2 的前 10 条成绩");
}

#[test]
fn constant_type_is_a_static_tuple() {
    let spec: (&'static str, bool) = __AURORIX_CMD_BAN;
    assert_eq!(spec.0, "/ban");
    assert!(spec.1, "admin 标记要是 true");
}
