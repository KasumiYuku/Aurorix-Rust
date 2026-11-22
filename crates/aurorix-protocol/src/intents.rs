pub const DEFAULT_INTENTS: [&str; 13] = [
    "GROUP_AT_MESSAGE_CREATE",
    "GROUP_MESSAGE_CREATE",
    "C2C_MESSAGE_CREATE",
    "INTERACTION_CREATE",
    "GROUP_JOIN_REQUEST",
    "GROUP_MEMBER_ADD",
    "GROUP_MEMBER_REMOVE",
    "MESSAGE_AUDIT_PASS",
    "MESSAGE_AUDIT_REJECT",
    "GROUP_ADD_ROBOT",
    "GROUP_DEL_ROBOT",
    "GROUP_MSG_RECEIVE",
    "GROUP_MSG_REJECT",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IntentSet {
    pub unknown: Vec<String>,
    enabled: [bool; 13],
}

impl IntentSet {
    pub fn from_names(names: &[String]) -> Self {
        let mut selected = Self::empty();
        for name in names {
            match DEFAULT_INTENTS.iter().position(|known| known == name) {
                Some(index) => selected.enabled[index] = true,
                None => {
                    if !selected.unknown.contains(name) {
                        selected.unknown.push(name.clone());
                    }
                }
            }
        }
        selected
    }

    pub fn all() -> Self {
        let mut selected = Self::empty();
        for slot in selected.enabled.iter_mut() {
            *slot = true;
        }
        selected
    }

    pub fn empty() -> Self {
        Self {
            unknown: Vec::new(),
            enabled: [false; 13],
        }
    }

    pub fn names(&self) -> Vec<String> {
        DEFAULT_INTENTS
            .iter()
            .zip(self.enabled.iter())
            .filter(|(_, enabled)| **enabled)
            .map(|(name, _)| (*name).to_string())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.enabled.iter().filter(|slot| **slot).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains(&self, name: &str) -> bool {
        match DEFAULT_INTENTS.iter().position(|known| *known == name) {
            Some(index) => self.enabled[index],
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_intents_keep_platform_order() {
        assert_eq!(DEFAULT_INTENTS.len(), 13);
        assert_eq!(DEFAULT_INTENTS[0], "GROUP_AT_MESSAGE_CREATE");
        assert_eq!(DEFAULT_INTENTS[2], "C2C_MESSAGE_CREATE");
        assert_eq!(DEFAULT_INTENTS[12], "GROUP_MSG_REJECT");
        assert_eq!(IntentSet::all().names().len(), 13);
    }

    #[test]
    fn names_are_reported_in_default_order() {
        let input = vec![
            "GROUP_MSG_REJECT".to_string(),
            "GROUP_AT_MESSAGE_CREATE".to_string(),
            "MESSAGE_AUDIT_PASS".to_string(),
        ];
        let set = IntentSet::from_names(&input);
        assert_eq!(
            set.names(),
            vec![
                "GROUP_AT_MESSAGE_CREATE".to_string(),
                "MESSAGE_AUDIT_PASS".to_string(),
                "GROUP_MSG_REJECT".to_string(),
            ],
            "顺序必须跟随默认表"
        );
        assert_eq!(set.len(), 3);
        assert!(!set.is_empty());
        assert!(set.contains("MESSAGE_AUDIT_PASS"));
        assert!(!set.contains("INTERACTION_CREATE"));
    }

    #[test]
    fn unknown_names_are_recorded_not_rejected() {
        let input = vec![
            "GROUP_AT_MESSAGE_CREATE".to_string(),
            "FUTURE_EVENT".to_string(),
            "FUTURE_EVENT".to_string(),
            "MESSAGE_AUDIT_REJECT".to_string(),
        ];
        let set = IntentSet::from_names(&input);
        assert_eq!(
            set.unknown,
            vec!["FUTURE_EVENT".to_string()],
            "未知名字要记下来"
        );
        assert_eq!(
            set.names(),
            vec![
                "GROUP_AT_MESSAGE_CREATE".to_string(),
                "MESSAGE_AUDIT_REJECT".to_string(),
            ]
        );
        assert!(!set.contains("FUTURE_EVENT"));
    }

    #[test]
    fn empty_input_yields_empty_set() {
        let set = IntentSet::from_names(&[]);
        assert!(set.is_empty());
        assert!(set.names().is_empty());
        assert!(set.unknown.is_empty());
    }
}
