pub const AUTH_FAIL: u16 = 4004;
pub const SESSION_GONE: u16 = 4006;
pub const INVALID_SEQ: u16 = 4007;
pub const BOT_OFFLINE: u16 = 4914;
pub const BOT_BANNED: u16 = 4915;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    Reconnect,
    Reidentify,
    AuthFail,
    Stop,
}

pub fn can_resume(code: u16) -> bool {
    if code == BOT_OFFLINE {
        return false;
    }
    if code == BOT_BANNED {
        return false;
    }
    true
}

pub fn need_reidentify(code: u16) -> bool {
    if code == AUTH_FAIL {
        return true;
    }
    if code == SESSION_GONE {
        return true;
    }
    if code == INVALID_SEQ {
        return true;
    }
    false
}

pub fn classify(code: u16) -> CloseAction {
    if !can_resume(code) {
        return CloseAction::Stop;
    }
    if code == AUTH_FAIL {
        return CloseAction::AuthFail;
    }
    if need_reidentify(code) {
        return CloseAction::Reidentify;
    }
    CloseAction::Reconnect
}

pub fn is_permanent(code: u16) -> bool {
    classify(code) == CloseAction::Stop
}
