pub mod announce;
pub mod bilibili;
pub mod chatglm;
pub mod chinaunicom;
pub mod cnb;
pub mod homework;
pub mod kurobbs;
pub mod mihoyo;
pub mod qqbot;
pub mod qqgame;
pub mod qqstream;
pub mod qzone;
pub mod sgame;
pub mod util;

use crate::registry::ProviderEntry;

pub fn builtin_entries() -> Vec<ProviderEntry> {
    vec![
        announce::entry(),
        bilibili::entry(),
        chatglm::entry(),
        chinaunicom::entry(),
        cnb::entry(),
        homework::entry(),
        kurobbs::entry(),
        mihoyo::entry(),
        qqbot::entry(),
        qqgame::entry(),
        qqstream::entry(),
        qzone::entry(),
        sgame::entry(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_entries_cover_the_go_registry() {
        let entries = builtin_entries();
        assert_eq!(entries.len(), 13);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name).collect();
        assert!(names.contains(&"qqbot"));
        assert!(names.contains(&"chatglm"));
        assert_eq!(
            names.iter().filter(|name| **name == "qqbot").count(),
            1,
            "注册名必须唯一"
        );
        let chatglm = entries
            .iter()
            .find(|entry| entry.name == "chatglm")
            .unwrap();
        assert!(chatglm.fields.is_empty());
        assert!(!chatglm.default_on);
    }
}
