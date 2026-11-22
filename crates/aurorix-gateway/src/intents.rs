use aurorix_protocol::IntentSet;

use crate::error::GatewayError;

pub const GUILDS: u64 = 1 << 0;
pub const GUILD_MEMBERS: u64 = 1 << 1;
pub const GUILD_MESSAGES: u64 = 1 << 9;
pub const DIRECT_MESSAGE: u64 = 1 << 12;
pub const GROUP_MEMBERS: u64 = 1 << 24;
pub const USER_MESSAGE: u64 = 1 << 25;
pub const INTERACTIONS: u64 = 1 << 26;
pub const MESSAGE_AUDIT: u64 = 1 << 27;
pub const PUBLIC_MESSAGES: u64 = 1 << 30;

pub const DEFAULT_INTENT_BITS: u64 = USER_MESSAGE | INTERACTIONS | GROUP_MEMBERS | MESSAGE_AUDIT;

pub const GUILD_INTENT_BITS: u64 =
    GUILDS | GUILD_MEMBERS | GUILD_MESSAGES | DIRECT_MESSAGE | PUBLIC_MESSAGES;

pub const EVENT_INTENTS: [(&str, u64); 28] = [
    ("GROUP_AT_MESSAGE_CREATE", USER_MESSAGE),
    ("GROUP_MESSAGE_CREATE", USER_MESSAGE),
    ("C2C_MESSAGE_CREATE", USER_MESSAGE),
    ("INTERACTION_CREATE", INTERACTIONS),
    ("GROUP_JOIN_REQUEST", USER_MESSAGE),
    ("GROUP_MEMBER_ADD", GROUP_MEMBERS),
    ("GROUP_MEMBER_REMOVE", GROUP_MEMBERS),
    ("GROUP_ADD_ROBOT", USER_MESSAGE),
    ("GROUP_DEL_ROBOT", USER_MESSAGE),
    ("GROUP_MSG_RECEIVE", USER_MESSAGE),
    ("GROUP_MSG_REJECT", USER_MESSAGE),
    ("C2C_MSG_RECEIVE", USER_MESSAGE),
    ("C2C_MSG_REJECT", USER_MESSAGE),
    ("FRIEND_ADD", USER_MESSAGE),
    ("FRIEND_DEL", USER_MESSAGE),
    ("MESSAGE_AUDIT_PASS", MESSAGE_AUDIT),
    ("MESSAGE_AUDIT_REJECT", MESSAGE_AUDIT),
    ("GUILD_CREATE", GUILDS),
    ("GUILD_UPDATE", GUILDS),
    ("GUILD_DELETE", GUILDS),
    ("CHANNEL_CREATE", GUILDS),
    ("CHANNEL_UPDATE", GUILDS),
    ("CHANNEL_DELETE", GUILDS),
    ("GUILD_MEMBER_ADD", GUILD_MEMBERS),
    ("GUILD_MEMBER_REMOVE", GUILD_MEMBERS),
    ("AT_MESSAGE_CREATE", PUBLIC_MESSAGES),
    ("MESSAGE_CREATE", GUILD_MESSAGES),
    ("DIRECT_MESSAGE_CREATE", DIRECT_MESSAGE),
];

pub const INTENT_EVENTS: [&str; 28] = [
    "GROUP_AT_MESSAGE_CREATE",
    "GROUP_MESSAGE_CREATE",
    "C2C_MESSAGE_CREATE",
    "INTERACTION_CREATE",
    "GROUP_JOIN_REQUEST",
    "GROUP_MEMBER_ADD",
    "GROUP_MEMBER_REMOVE",
    "GROUP_ADD_ROBOT",
    "GROUP_DEL_ROBOT",
    "GROUP_MSG_RECEIVE",
    "GROUP_MSG_REJECT",
    "C2C_MSG_RECEIVE",
    "C2C_MSG_REJECT",
    "FRIEND_ADD",
    "FRIEND_DEL",
    "MESSAGE_AUDIT_PASS",
    "MESSAGE_AUDIT_REJECT",
    "AT_MESSAGE_CREATE",
    "MESSAGE_CREATE",
    "DIRECT_MESSAGE_CREATE",
    "GUILD_CREATE",
    "GUILD_UPDATE",
    "GUILD_DELETE",
    "CHANNEL_CREATE",
    "CHANNEL_UPDATE",
    "CHANNEL_DELETE",
    "GUILD_MEMBER_ADD",
    "GUILD_MEMBER_REMOVE",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Webhook,
    Websocket,
}

impl Protocol {
    pub fn parse(raw: &str) -> Option<Protocol> {
        match raw {
            "webhook" => Some(Protocol::Webhook),
            "websocket" => Some(Protocol::Websocket),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Webhook => "webhook",
            Protocol::Websocket => "websocket",
        }
    }

    pub fn requires_websocket(self, names: &[String]) -> bool {
        self == Protocol::Websocket || requires_websocket(names)
    }
}

pub fn intent_bit(name: &str) -> u64 {
    let mut result = 0u64;
    for (known, bit) in EVENT_INTENTS {
        if known == name {
            result = bit;
            break;
        }
    }
    result
}

pub fn bits_for_names(names: &[String]) -> u64 {
    let mut bits = 0u64;
    for name in names {
        bits |= intent_bit(name);
    }
    if bits == 0 {
        DEFAULT_INTENT_BITS
    } else {
        bits
    }
}

pub fn bits_for_set(set: &IntentSet) -> u64 {
    let mut names = set.names();
    names.extend(set.unknown.iter().cloned());
    bits_for_names(&names)
}

pub fn requires_websocket(names: &[String]) -> bool {
    for name in names {
        if intent_bit(name) & GUILD_INTENT_BITS != 0 {
            return true;
        }
    }
    false
}

pub fn ensure_protocol(protocol: Protocol, names: &[String]) -> Result<(), GatewayError> {
    if protocol == Protocol::Webhook && requires_websocket(names) {
        return Err(GatewayError::Config(
            "订阅了频道事件, 但协议是 webhook: 频道消息要求机器人保持 WebSocket 在线, 请把 protocol 改成 websocket"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn unknown_names(names: &[String]) -> Vec<String> {
    let mut unknown = Vec::new();
    for name in names {
        if intent_bit(name) == 0 && !unknown.contains(name) {
            unknown.push(name.clone());
        }
    }
    unknown
}
