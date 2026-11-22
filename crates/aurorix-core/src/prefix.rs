use std::sync::Arc;
use std::sync::LazyLock;

use arc_swap::ArcSwap;

pub fn default_prefix_chars() -> Vec<String> {
    vec!["/".to_string(), "#".to_string(), String::new()]
}

#[derive(Debug)]
pub struct PrefixBook {
    chars: ArcSwap<Vec<String>>,
}

impl Default for PrefixBook {
    fn default() -> Self {
        Self {
            chars: ArcSwap::from_pointee(default_prefix_chars()),
        }
    }
}

impl PrefixBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn global() -> &'static PrefixBook {
        static BOOK: LazyLock<PrefixBook> = LazyLock::new(PrefixBook::new);
        &BOOK
    }

    pub fn get(&self) -> Arc<Vec<String>> {
        self.chars.load_full()
    }

    pub fn set(&self, chars: Vec<String>) {
        let next = if chars.is_empty() {
            default_prefix_chars()
        } else {
            chars
        };
        self.chars.store(Arc::new(next));
    }

    pub fn has_bare(&self) -> bool {
        self.get().iter().any(|prefix| prefix.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_go_order() {
        let book = PrefixBook::new();
        assert_eq!(
            *book.get(),
            vec!["/".to_string(), "#".to_string(), String::new()]
        );
        assert!(book.has_bare());
    }

    #[test]
    fn set_empty_restores_defaults() {
        let book = PrefixBook::new();
        book.set(vec!["!".to_string()]);
        assert_eq!(*book.get(), vec!["!".to_string()]);
        assert!(!book.has_bare());
        book.set(Vec::new());
        assert!(book.has_bare());
    }
}
