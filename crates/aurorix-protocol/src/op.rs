#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum WebSocketOp {
    Dispatch = 0,
    Heartbeat = 1,
    Identify = 2,
    Resume = 6,
    Reconnect = 7,
    InvalidSession = 9,
    Hello = 10,
    HeartbeatAck = 11,
}

impl WebSocketOp {
    pub const ALL: [WebSocketOp; 8] = [
        WebSocketOp::Dispatch,
        WebSocketOp::Heartbeat,
        WebSocketOp::Identify,
        WebSocketOp::Resume,
        WebSocketOp::Reconnect,
        WebSocketOp::InvalidSession,
        WebSocketOp::Hello,
        WebSocketOp::HeartbeatAck,
    ];

    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(WebSocketOp::Dispatch),
            1 => Some(WebSocketOp::Heartbeat),
            2 => Some(WebSocketOp::Identify),
            6 => Some(WebSocketOp::Resume),
            7 => Some(WebSocketOp::Reconnect),
            9 => Some(WebSocketOp::InvalidSession),
            10 => Some(WebSocketOp::Hello),
            11 => Some(WebSocketOp::HeartbeatAck),
            _ => None,
        }
    }

    pub const fn code(self) -> i32 {
        self as i32
    }

    pub const fn label(self) -> &'static str {
        match self {
            WebSocketOp::Dispatch => "dispatch",
            WebSocketOp::Heartbeat => "heartbeat",
            WebSocketOp::Identify => "identify",
            WebSocketOp::Resume => "resume",
            WebSocketOp::Reconnect => "reconnect",
            WebSocketOp::InvalidSession => "invalid session",
            WebSocketOp::Hello => "hello",
            WebSocketOp::HeartbeatAck => "heartbeat ack",
        }
    }
}

impl From<WebSocketOp> for i32 {
    fn from(op: WebSocketOp) -> Self {
        op.code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_zero_to_eleven_match_platform_table() {
        let expected = [
            Some(WebSocketOp::Dispatch),
            Some(WebSocketOp::Heartbeat),
            Some(WebSocketOp::Identify),
            None,
            None,
            None,
            Some(WebSocketOp::Resume),
            Some(WebSocketOp::Reconnect),
            None,
            Some(WebSocketOp::InvalidSession),
            Some(WebSocketOp::Hello),
            Some(WebSocketOp::HeartbeatAck),
        ];
        for (code, want) in (0..=11).zip(expected) {
            assert_eq!(WebSocketOp::from_code(code), want, "码 {code} 解析结果不对");
        }
    }

    #[test]
    fn illegal_codes_are_rejected() {
        for code in [-1, 3, 4, 5, 8, 12, 13, 100, i32::MAX] {
            assert!(
                WebSocketOp::from_code(code).is_none(),
                "码 {code} 不该被接受"
            );
        }
    }

    #[test]
    fn all_variants_round_trip() {
        assert_eq!(WebSocketOp::ALL.len(), 8);
        for op in WebSocketOp::ALL {
            assert_eq!(WebSocketOp::from_code(op.code()), Some(op), "往返失败");
            assert!(!op.label().is_empty(), "标签不能为空");
        }
        assert_eq!(WebSocketOp::Dispatch.code(), 0);
        assert_eq!(WebSocketOp::Hello.code(), 10);
        assert_eq!(i32::from(WebSocketOp::HeartbeatAck), 11);
    }
}
