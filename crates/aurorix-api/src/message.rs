use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use aurorix_buttons::Keyboard;
use aurorix_core::Target;
use aurorix_protocol::MessageKind;
use serde::Deserialize;
use serde_json::Value;

use crate::error::ApiError;

pub const MAX_PASSIVE_SEQ: u32 = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anchor {
    pub msg_id: String,
    pub event_id: String,
}

impl Anchor {
    pub fn new(msg_id: impl Into<String>, event_id: impl Into<String>) -> Self {
        Self {
            msg_id: msg_id.into(),
            event_id: event_id.into(),
        }
    }

    pub fn from_message(msg_id: impl Into<String>) -> Self {
        Self {
            msg_id: msg_id.into(),
            event_id: String::new(),
        }
    }

    pub fn from_event(event_id: impl Into<String>) -> Self {
        Self {
            msg_id: String::new(),
            event_id: event_id.into(),
        }
    }

    pub fn empty(&self) -> bool {
        self.msg_id.is_empty() && self.event_id.is_empty()
    }
}

#[derive(Debug, Default)]
pub struct SeqBudget {
    sent: AtomicU32,
    spent: AtomicBool,
}

impl SeqBudget {
    pub fn new() -> Self {
        Self {
            sent: AtomicU32::new(0),
            spent: AtomicBool::new(false),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn count(&self) -> Result<u32, ApiError> {
        if self.spent.load(Ordering::SeqCst) {
            return Err(ApiError::ReplyBudgetExhausted {
                limit: MAX_PASSIVE_SEQ,
            });
        }
        let next = self.sent.fetch_add(1, Ordering::SeqCst) + 1;
        if next > MAX_PASSIVE_SEQ {
            self.spent.store(true, Ordering::SeqCst);
            return Err(ApiError::ReplyBudgetExhausted {
                limit: MAX_PASSIVE_SEQ,
            });
        }
        Ok(next)
    }

    pub fn spent(&self) -> bool {
        self.spent.load(Ordering::SeqCst)
    }

    pub fn sent(&self) -> u32 {
        self.sent.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone)]
pub struct Outbound {
    pub kind: MessageKind,
    pub content: String,
    pub media: Option<Value>,
    pub keyboard: Option<Value>,
    pub anchor: Anchor,
    pub budget: Option<Arc<SeqBudget>>,
    pub active_fallback: bool,
    pub initiative: bool,
    pub reference: Option<String>,
}

impl Outbound {
    pub fn text(content: impl Into<String>) -> Self {
        Self::bare(content, MessageKind::PlainText)
    }

    pub fn markdown(content: impl Into<String>) -> Self {
        Self::bare(content, MessageKind::Markdown)
    }

    pub fn media(content: impl Into<String>, media: Value) -> Self {
        let mut outbound = Self::bare(content, MessageKind::Media);
        outbound.media = Some(media);
        outbound
    }

    fn bare(content: impl Into<String>, kind: MessageKind) -> Self {
        Self {
            kind,
            content: content.into(),
            media: None,
            keyboard: None,
            anchor: Anchor::default(),
            budget: None,
            active_fallback: true,
            initiative: false,
            reference: None,
        }
    }

    pub fn with_keyboard(mut self, keyboard: &Keyboard) -> Result<Self, ApiError> {
        let raw = keyboard
            .to_json()
            .map_err(|err| ApiError::Keyboard(err.to_string()))?;
        self.keyboard = Some(serde_json::from_slice(&raw)?);
        Ok(self)
    }

    pub fn with_keyboard_json(mut self, keyboard: Value) -> Self {
        self.keyboard = Some(keyboard);
        self
    }

    pub fn quote_to(mut self, message_id: impl Into<String>) -> Self {
        self.reference = Some(message_id.into());
        self
    }

    pub fn with_anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn with_msg_id(self, msg_id: impl Into<String>) -> Self {
        self.with_anchor(Anchor::from_message(msg_id))
    }

    pub fn with_event_id(self, event_id: impl Into<String>) -> Self {
        self.with_anchor(Anchor::from_event(event_id))
    }

    pub fn with_budget(mut self, budget: Arc<SeqBudget>) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn disable_active_fallback(mut self) -> Self {
        self.active_fallback = false;
        self
    }

    pub fn as_initiative(mut self) -> Self {
        self.initiative = true;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct SendResult {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub audit_id: String,
    #[serde(default)]
    pub timestamp: String,
}

impl SendResult {
    pub fn message_id(&self) -> Option<String> {
        if self.id.is_empty() {
            None
        } else {
            Some(self.id.clone())
        }
    }

    pub fn audit(&self) -> Option<String> {
        if self.audit_id.is_empty() {
            None
        } else {
            Some(self.audit_id.clone())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOutcome {
    pub message_id: Option<String>,
    pub audit_id: Option<String>,
    pub target: Target,
    pub delivered_active: bool,
    pub fallback: bool,
}

impl SendOutcome {
    pub fn recallable(&self) -> bool {
        self.message_id.is_some() && !self.delivered_active
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryStep {
    pub target: Target,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ResolvedAnchor {
    pub msg_id: Option<String>,
    pub event_id: Option<String>,
    pub seq: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_starts_from_one_and_stops_at_five() {
        let budget = SeqBudget::new();
        assert_eq!(budget.count().unwrap(), 1, "msg_seq 必须从 1 起");
        assert_eq!(budget.count().unwrap(), 2);
        assert_eq!(budget.count().unwrap(), 3);
        assert_eq!(budget.count().unwrap(), 4);
        assert_eq!(budget.count().unwrap(), 5);
        assert_eq!(budget.sent(), 5);
        let err = budget.count().unwrap_err();
        assert_eq!(
            err,
            ApiError::ReplyBudgetExhausted {
                limit: MAX_PASSIVE_SEQ
            }
        );
        assert!(budget.spent(), "第 6 条之后预算算用光");
        assert!(budget.count().is_err());
    }

    #[test]
    fn seq_counter_is_atomic_under_concurrency() {
        let budget = SeqBudget::shared();
        let mut handles = Vec::new();
        for _ in 0..5 {
            let clone = Arc::clone(&budget);
            handles.push(std::thread::spawn(move || clone.count()));
        }
        let mut values: Vec<u32> = handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap().ok())
            .collect();
        values.sort_unstable();
        assert_eq!(values, vec![1, 2, 3, 4, 5], "并发下序号必须唯一且连续");
    }

    #[test]
    fn anchor_helpers_keep_single_field() {
        assert!(Anchor::default().empty());
        let by_message = Anchor::from_message("m1");
        assert_eq!(by_message.msg_id, "m1");
        assert!(by_message.event_id.is_empty());
        let by_event = Anchor::from_event("e1");
        assert!(by_event.msg_id.is_empty());
        assert!(!by_event.empty());
        let both = Anchor::new("m", "e");
        assert_eq!(both, Anchor::new("m", "e"));
    }

    #[test]
    fn outbound_builders_are_fluent() {
        let budget = SeqBudget::shared();
        let outbound = Outbound::markdown("# 标题")
            .with_msg_id("m1")
            .with_budget(Arc::clone(&budget))
            .as_initiative()
            .disable_active_fallback();
        assert_eq!(outbound.kind, MessageKind::Markdown);
        assert_eq!(outbound.anchor.msg_id, "m1");
        assert!(outbound.initiative);
        assert!(!outbound.active_fallback);
        assert!(outbound.budget.is_some());
        let media = Outbound::media("图", serde_json::json!({"file_info": "f"}));
        assert_eq!(media.kind, MessageKind::Media);
        assert!(media.media.is_some());
        let plain = Outbound::text("你好");
        assert_eq!(plain.kind, MessageKind::PlainText);
        assert!(plain.media.is_none());
        assert!(plain.anchor.empty());
    }

    #[test]
    fn keyboard_json_is_attached() {
        let keyboard = aurorix_buttons::Builder::new()
            .command("pick", "点我", "/pick", Vec::new())
            .build()
            .expect("键盘要能构造");
        let outbound = Outbound::markdown("x").with_keyboard(&keyboard).unwrap();
        let attached = outbound.keyboard.expect("键盘要挂上");
        assert!(attached.get("content").is_some(), "键盘必须是 content 形态");
        assert!(attached["content"].get("rows").is_some());
    }

    #[test]
    fn send_result_empties_become_none() {
        let empty = SendResult::default();
        assert_eq!(empty.message_id(), None);
        assert_eq!(empty.audit(), None);
        let full = SendResult {
            id: String::from("m1"),
            audit_id: String::from("a1"),
            timestamp: String::from("1700000000"),
        };
        assert_eq!(full.message_id(), Some(String::from("m1")));
        assert_eq!(full.audit(), Some(String::from("a1")));
        let outcome = SendOutcome {
            message_id: Some(String::from("m1")),
            audit_id: None,
            target: Target::new(aurorix_core::Scene::Group, "g1"),
            delivered_active: false,
            fallback: false,
        };
        assert!(outcome.recallable());
        let mut active = outcome.clone();
        active.delivered_active = true;
        assert!(!active.recallable(), "主动消息不撤回");
    }
}
