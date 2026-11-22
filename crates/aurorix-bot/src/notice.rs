use aurorix_core::{Scene, Target};
use aurorix_gateway::ProtocolEvent;
use serde_json::Value;

pub const AVATAR_SIZE: usize = 140;

pub const AVATAR_SHOW_PX: usize = 40;

pub const NO_PERMISSION_CODE: i32 = 40034105;

pub const AVATAR_BASE: &str = "https://thirdqq.qlogo.cn/qqapp";

pub const GROUP_MEMBER_ADD: &str = "GROUP_MEMBER_ADD";

pub const GROUP_MEMBER_REMOVE: &str = "GROUP_MEMBER_REMOVE";

pub const GUILD_MEMBER_ADD: &str = "GUILD_MEMBER_ADD";

pub const GUILD_MEMBER_REMOVE: &str = "GUILD_MEMBER_REMOVE";

pub const GROUP_MSG_RECEIVE: &str = "GROUP_MSG_RECEIVE";

pub const GROUP_MSG_REJECT: &str = "GROUP_MSG_REJECT";

pub const C2C_MSG_RECEIVE: &str = "C2C_MSG_RECEIVE";

pub const C2C_MSG_REJECT: &str = "C2C_MSG_REJECT";

pub const MEMBER_EVENTS: [&str; 4] = [
    GROUP_MEMBER_ADD,
    GROUP_MEMBER_REMOVE,
    GUILD_MEMBER_ADD,
    GUILD_MEMBER_REMOVE,
];

pub const FRIEND_EVENTS: [&str; 2] = [crate::event::FRIEND_ADD, crate::event::FRIEND_DEL];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    MemberAdd,
    MemberRemove,
    FriendAdd,
    FriendRemove,
    GuildMemberAdd,
    GuildMemberRemove,
}

impl NoticeKind {
    pub fn of(event_type: &str) -> Option<Self> {
        match event_type {
            GROUP_MEMBER_ADD => Some(NoticeKind::MemberAdd),
            GROUP_MEMBER_REMOVE => Some(NoticeKind::MemberRemove),
            GUILD_MEMBER_ADD => Some(NoticeKind::GuildMemberAdd),
            GUILD_MEMBER_REMOVE => Some(NoticeKind::GuildMemberRemove),
            crate::event::FRIEND_ADD => Some(NoticeKind::FriendAdd),
            crate::event::FRIEND_DEL => Some(NoticeKind::FriendRemove),
            _ => None,
        }
    }

    pub const fn added(self) -> bool {
        matches!(
            self,
            NoticeKind::MemberAdd | NoticeKind::FriendAdd | NoticeKind::GuildMemberAdd
        )
    }

    pub const fn friend(self) -> bool {
        matches!(self, NoticeKind::FriendAdd | NoticeKind::FriendRemove)
    }

    pub const fn label(self) -> &'static str {
        match self {
            NoticeKind::MemberAdd => "成员入群",
            NoticeKind::MemberRemove => "成员退群",
            NoticeKind::FriendAdd => "好友添加",
            NoticeKind::FriendRemove => "好友删除",
            NoticeKind::GuildMemberAdd => "频道成员加入",
            NoticeKind::GuildMemberRemove => "频道成员退出",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeEvent {
    pub kind: NoticeKind,
    pub scene: Scene,
    pub target: Target,
    pub user: String,
    pub event_id: String,
    pub message_id: String,
    pub timestamp: i64,
}

impl NoticeEvent {
    pub fn parse(event: &ProtocolEvent) -> Option<Self> {
        let kind = NoticeKind::of(event.event_type.as_deref()?)?;
        let data = &event.data;
        let event_id = event.id.clone().unwrap_or_default();
        let message_id = text(data, "message_id").unwrap_or_default();
        let timestamp = data
            .get("timestamp")
            .and_then(Value::as_i64)
            .or_else(|| {
                data.get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(|raw| raw.parse::<i64>().ok())
            })
            .unwrap_or_default();
        match kind {
            NoticeKind::MemberAdd | NoticeKind::MemberRemove => {
                let group = text(data, "group_openid")
                    .filter(|id| !id.is_empty())
                    .or_else(|| text(data, "group_id").filter(|id| !id.is_empty()))?;
                let user = text(data, "member_openid")
                    .filter(|id| !id.is_empty())
                    .or_else(|| text(data, "op_member_openid").filter(|id| !id.is_empty()))
                    .or_else(|| nested_text(data, "author", "union_openid"))
                    .or_else(|| text(data, "openid").filter(|id| !id.is_empty()))?;
                Some(Self {
                    kind,
                    scene: Scene::Group,
                    target: Target::new(Scene::Group, group).with_user(user.clone()),
                    user,
                    event_id,
                    message_id,
                    timestamp,
                })
            }
            NoticeKind::GuildMemberAdd | NoticeKind::GuildMemberRemove => {
                let guild = text(data, "guild_id")
                    .filter(|id| !id.is_empty())
                    .or_else(|| text(data, "channel_id").filter(|id| !id.is_empty()))?;
                let user = text(data, "user_id")
                    .filter(|id| !id.is_empty())
                    .or_else(|| nested_text(data, "user", "id"))
                    .or_else(|| nested_text(data, "author", "id"))
                    .or_else(|| text(data, "openid").filter(|id| !id.is_empty()))?;
                Some(Self {
                    kind,
                    scene: Scene::Channel,
                    target: Target::new(Scene::Channel, guild).with_user(user.clone()),
                    user,
                    event_id,
                    message_id,
                    timestamp,
                })
            }
            NoticeKind::FriendAdd | NoticeKind::FriendRemove => {
                let user = text(data, "openid")
                    .filter(|id| !id.is_empty())
                    .or_else(|| nested_text(data, "user", "openid"))
                    .or_else(|| nested_text(data, "author", "union_openid"))?;
                Some(Self {
                    kind,
                    scene: Scene::C2c,
                    target: Target::new(Scene::C2c, user.clone()).with_user(user.clone()),
                    user,
                    event_id,
                    message_id,
                    timestamp,
                })
            }
        }
    }

    pub fn can_reply(&self) -> bool {
        self.kind.added() && !self.kind.friend() && !self.event_id.is_empty() && !self.scene.guild()
    }
}

pub fn avatar_url(app_id: &str, user_id: &str, size: usize) -> Option<String> {
    if app_id.is_empty() || user_id.is_empty() {
        return None;
    }
    let size = if size == 0 { 640 } else { size };
    Some(format!("{AVATAR_BASE}/{app_id}/{user_id}/{size}"))
}

pub fn compose(avatar: &str, user_id: &str, added: bool, text: &str, extra: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut head = String::new();
    if !avatar.is_empty() {
        head = format!("![cornerRadius=100 #{AVATAR_SHOW_PX}px #{AVATAR_SHOW_PX}px]({avatar})");
    }
    if added && !user_id.is_empty() {
        if !head.is_empty() {
            head.push(' ');
        }
        head.push_str(&mention(user_id));
    }
    let mut out = String::new();
    if !head.is_empty() {
        out.push_str("##");
        out.push_str(&head);
        out.push('\n');
    }
    out.push_str(text);
    if !extra.is_empty() {
        out.push('\n');
        out.push_str(extra);
    }
    out
}

pub fn mention(user_id: &str) -> String {
    if user_id.is_empty() {
        return String::new();
    }
    format!("<@{user_id}>")
}

pub fn pick_line(pool: &[String]) -> String {
    let lines: Vec<&str> = pool
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    match lines.len() {
        0 => String::new(),
        1 => lines[0].to_string(),
        count => {
            let index = pick_index(count);
            lines[index].to_string()
        }
    }
}

pub fn pick_index(count: usize) -> usize {
    if count <= 1 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() as usize)
        .unwrap_or(0);
    let counter = NEXT_PICK.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as usize;
    (nanos ^ counter.wrapping_mul(0x9e37_79b9)) % count
}

static NEXT_PICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub struct ReceiveGate {
    pub group: String,
    pub allowed: bool,
}

pub fn receive_gate(event: &ProtocolEvent) -> Option<ReceiveGate> {
    let kind = event.event_type.as_deref()?;
    let allowed = match kind {
        GROUP_MSG_RECEIVE | C2C_MSG_RECEIVE => true,
        GROUP_MSG_REJECT | C2C_MSG_REJECT => false,
        _ => return None,
    };
    let data = &event.data;
    let group = text(data, "group_openid")
        .filter(|id| !id.is_empty())
        .or_else(|| text(data, "openid").filter(|id| !id.is_empty()))
        .or_else(|| text(data, "group_id").filter(|id| !id.is_empty()))?;
    Some(ReceiveGate { group, allowed })
}

fn text(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(Value::as_str).map(str::to_string)
}

fn nested_text(data: &Value, outer: &str, inner: &str) -> Option<String> {
    data.get(outer)
        .and_then(|value| text(value, inner))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn notice(event_type: &str, data: Value) -> ProtocolEvent {
        ProtocolEvent {
            id: Some(String::from("evt-1")),
            op: 0,
            event_type: Some(event_type.to_string()),
            seq: Some(3),
            data,
            raw: String::new(),
        }
    }

    #[test]
    fn member_add_carries_group_and_member() {
        let parsed = NoticeEvent::parse(&notice(
            GROUP_MEMBER_ADD,
            json!({
                "group_openid": "g1",
                "member_openid": "u1",
                "message_id": "m1",
                "timestamp": 1700000000
            }),
        ))
        .expect("入群事件应当解析成功");
        assert_eq!(parsed.kind, NoticeKind::MemberAdd);
        assert_eq!(parsed.scene, Scene::Group);
        assert_eq!(parsed.target.id, "g1");
        assert_eq!(parsed.user, "u1");
        assert_eq!(parsed.message_id, "m1");
        assert_eq!(parsed.timestamp, 1_700_000_000);
        assert!(parsed.can_reply(), "带 event_id 的入群事件可以被动回复");
    }

    #[test]
    fn member_remove_falls_back_to_author_union_openid() {
        let parsed = NoticeEvent::parse(&notice(
            GROUP_MEMBER_REMOVE,
            json!({ "group_openid": "g1", "author": { "union_openid": "u9" } }),
        ))
        .expect("退群事件应当解析成功");
        assert_eq!(parsed.kind, NoticeKind::MemberRemove);
        assert_eq!(
            parsed.user, "u9",
            "缺失 member_openid 时要回落到 union_openid"
        );
        assert!(!parsed.kind.added());
    }

    #[test]
    fn only_added_members_get_a_passive_reply() {
        let removed = NoticeEvent::parse(&notice(
            GROUP_MEMBER_REMOVE,
            json!({ "group_openid": "g1", "member_openid": "u2" }),
        ))
        .expect("退群事件应当解析成功");
        assert!(
            !removed.can_reply(),
            "退群没有可用的被动回复凭据, 只能主动推送"
        );
    }

    #[test]
    fn friend_events_map_to_private_scene() {
        let added =
            NoticeEvent::parse(&notice(crate::event::FRIEND_ADD, json!({ "openid": "u7" })))
                .expect("加好友事件应当解析成功");
        assert_eq!(added.kind, NoticeKind::FriendAdd);
        assert_eq!(added.scene, Scene::C2c);
        assert_eq!(added.target.id, "u7");
        assert!(!added.can_reply(), "好友事件没有可用的被动回复凭据");
        let removed =
            NoticeEvent::parse(&notice(crate::event::FRIEND_DEL, json!({ "openid": "u7" })))
                .expect("删好友事件应当解析成功");
        assert_eq!(removed.kind, NoticeKind::FriendRemove);
        assert!(removed.kind.friend());
    }

    #[test]
    fn guild_member_events_use_guild_scene() {
        let parsed = NoticeEvent::parse(&notice(
            GUILD_MEMBER_ADD,
            json!({ "guild_id": "c1", "user": { "id": "u3" } }),
        ))
        .expect("频道成员事件应当解析成功");
        assert_eq!(parsed.scene, Scene::Channel);
        assert_eq!(parsed.target.id, "c1");
        assert_eq!(parsed.user, "u3");
        assert!(!parsed.can_reply(), "频道场景不走被动回复");
    }

    #[test]
    fn unknown_events_and_empty_payloads_are_rejected() {
        assert!(NoticeEvent::parse(&notice("GROUP_AT_MESSAGE_CREATE", json!({}))).is_none());
        assert!(
            NoticeEvent::parse(&notice(GROUP_MEMBER_ADD, json!({ "group_openid": "g1" })))
                .is_none()
        );
    }

    #[test]
    fn compose_puts_avatar_and_mention_on_the_header_line() {
        let body = compose(
            "https://thirdqq.qlogo.cn/qqapp/app1/u1/140",
            "u1",
            true,
            "欢迎来到这里",
            "> /help",
        );
        assert_eq!(
            body,
            "##![cornerRadius=100 #40px #40px](https://thirdqq.qlogo.cn/qqapp/app1/u1/140) <@u1>\n欢迎来到这里\n> /help",
            "头像与艾特要落在 ## 标题行, 附加内容另起一行"
        );
    }

    #[test]
    fn compose_skips_mention_when_the_member_left() {
        let body = compose("", "u1", false, "有人离开了群聊", "");
        assert_eq!(
            body, "有人离开了群聊",
            "退群不艾特, 也没有头像时不该出现标题行"
        );
    }

    #[test]
    fn pick_line_ignores_blank_entries() {
        let pool = vec![
            String::from("  "),
            String::from("只有这一条"),
            String::from(""),
        ];
        assert_eq!(pick_line(&pool), "只有这一条");
        assert_eq!(pick_line(&[]), "");
    }

    #[test]
    fn avatar_url_needs_both_ids() {
        assert_eq!(
            avatar_url("app1", "u1", AVATAR_SIZE).as_deref(),
            Some("https://thirdqq.qlogo.cn/qqapp/app1/u1/140")
        );
        assert_eq!(avatar_url("", "u1", AVATAR_SIZE), None);
        assert_eq!(avatar_url("app1", "", AVATAR_SIZE), None);
    }
}
