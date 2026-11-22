use std::fmt;

use serde::Deserialize;
use serde_json::Value;

use crate::op::WebSocketOp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EventKind {
    GroupAtMessageCreate,
    GroupMessageCreate,
    C2cMessageCreate,
    InteractionCreate,
    GroupJoinRequest,
    GroupMemberAdd,
    GroupMemberRemove,
    MessageAuditPass,
    MessageAuditReject,
    GroupAddRobot,
    GroupDelRobot,
    GroupMsgReceive,
    GroupMsgReject,
}

impl EventKind {
    pub const ALL: [EventKind; 13] = [
        EventKind::GroupAtMessageCreate,
        EventKind::GroupMessageCreate,
        EventKind::C2cMessageCreate,
        EventKind::InteractionCreate,
        EventKind::GroupJoinRequest,
        EventKind::GroupMemberAdd,
        EventKind::GroupMemberRemove,
        EventKind::MessageAuditPass,
        EventKind::MessageAuditReject,
        EventKind::GroupAddRobot,
        EventKind::GroupDelRobot,
        EventKind::GroupMsgReceive,
        EventKind::GroupMsgReject,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::GroupAtMessageCreate => "GROUP_AT_MESSAGE_CREATE",
            EventKind::GroupMessageCreate => "GROUP_MESSAGE_CREATE",
            EventKind::C2cMessageCreate => "C2C_MESSAGE_CREATE",
            EventKind::InteractionCreate => "INTERACTION_CREATE",
            EventKind::GroupJoinRequest => "GROUP_JOIN_REQUEST",
            EventKind::GroupMemberAdd => "GROUP_MEMBER_ADD",
            EventKind::GroupMemberRemove => "GROUP_MEMBER_REMOVE",
            EventKind::MessageAuditPass => "MESSAGE_AUDIT_PASS",
            EventKind::MessageAuditReject => "MESSAGE_AUDIT_REJECT",
            EventKind::GroupAddRobot => "GROUP_ADD_ROBOT",
            EventKind::GroupDelRobot => "GROUP_DEL_ROBOT",
            EventKind::GroupMsgReceive => "GROUP_MSG_RECEIVE",
            EventKind::GroupMsgReject => "GROUP_MSG_REJECT",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GatewayEvent {
    pub op: i32,
    #[serde(default)]
    pub s: Option<i64>,
    #[serde(default)]
    pub t: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub d: Value,
}

impl GatewayEvent {
    pub fn op_kind(&self) -> Option<WebSocketOp> {
        WebSocketOp::from_code(self.op)
    }

    pub fn is_dispatch(&self) -> bool {
        self.op_kind() == Some(WebSocketOp::Dispatch)
    }

    pub fn kind(&self) -> Option<EventKind> {
        self.t.as_deref().and_then(EventKind::parse)
    }

    pub fn event_id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn sequence(&self) -> Option<i64> {
        self.s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPATCH: &str = r#"{"op":0,"s":42,"t":"GROUP_AT_MESSAGE_CREATE","id":"e1","d":{"id":"m1","content":"/help","group_openid":"g1","author":{"member_openid":"u1"}}}"#;

    #[test]
    fn dispatch_frame_keeps_platform_fields() {
        let event: GatewayEvent = serde_json::from_str(DISPATCH).expect("分发事件要能解析");
        assert_eq!(event.op, 0);
        assert_eq!(event.op_kind(), Some(WebSocketOp::Dispatch));
        assert!(event.is_dispatch());
        assert_eq!(event.sequence(), Some(42));
        assert_eq!(event.t.as_deref(), Some("GROUP_AT_MESSAGE_CREATE"));
        assert_eq!(event.kind(), Some(EventKind::GroupAtMessageCreate));
        assert_eq!(event.event_id(), Some("e1"));
        assert_eq!(event.d["id"], "m1");
        assert_eq!(event.d["content"], "/help");
        assert_eq!(event.d["group_openid"], "g1");
        assert_eq!(event.d["author"]["member_openid"], "u1");
    }

    #[test]
    fn hello_frame_has_no_event_identity() {
        let event: GatewayEvent =
            serde_json::from_str(r#"{"op":10,"d":{"heartbeat_interval":30000}}"#)
                .expect("握手事件要能解析");
        assert_eq!(event.op_kind(), Some(WebSocketOp::Hello));
        assert!(!event.is_dispatch());
        assert_eq!(event.sequence(), None);
        assert_eq!(event.kind(), None);
        assert_eq!(event.event_id(), None);
        assert_eq!(event.d["heartbeat_interval"], 30000);
    }

    #[test]
    fn event_names_round_trip() {
        assert_eq!(EventKind::ALL.len(), 13);
        for kind in EventKind::ALL {
            assert_eq!(
                EventKind::parse(kind.as_str()),
                Some(kind),
                "事件名往返失败"
            );
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert_eq!(EventKind::parse("UNKNOWN_EVENT"), None);
        assert_eq!(
            EventKind::parse("GROUP_AT_MESSAGE_CREATE"),
            Some(EventKind::GroupAtMessageCreate)
        );
        assert_eq!(
            EventKind::parse("GROUP_MSG_REJECT"),
            Some(EventKind::GroupMsgReject)
        );
    }

    #[test]
    fn missing_data_defaults_to_null() {
        let event: GatewayEvent = serde_json::from_str(r#"{"op":11}"#).expect("心跳回应要能解析");
        assert_eq!(event.op_kind(), Some(WebSocketOp::HeartbeatAck));
        assert_eq!(event.d, Value::Null);
        assert_eq!(event.t, None);
    }
}
