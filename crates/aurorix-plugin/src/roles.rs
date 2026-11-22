use std::collections::BTreeMap;
use std::sync::Arc;

use aurorix_core::RoleRequired;

pub trait RoleResolver: Send + Sync {
    fn resolve(
        &self,
        user: &str,
        group: &str,
        platform: Option<RoleRequired>,
    ) -> Option<RoleRequired>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PlatformRoleResolver;

impl RoleResolver for PlatformRoleResolver {
    fn resolve(
        &self,
        _user: &str,
        _group: &str,
        platform: Option<RoleRequired>,
    ) -> Option<RoleRequired> {
        Some(platform.unwrap_or(RoleRequired::Member))
    }
}

#[derive(Debug, Clone, Default)]
pub struct StaticRoleResolver {
    users: BTreeMap<String, RoleRequired>,
    groups: BTreeMap<String, RoleRequired>,
}

impl StaticRoleResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_user(mut self, user: impl Into<String>, role: RoleRequired) -> Self {
        self.users.insert(user.into(), role);
        self
    }

    pub fn with_group(mut self, group: impl Into<String>, role: RoleRequired) -> Self {
        self.groups.insert(group.into(), role);
        self
    }

    pub fn set_user(&mut self, user: impl Into<String>, role: RoleRequired) {
        self.users.insert(user.into(), role);
    }

    pub fn shared(self) -> Arc<dyn RoleResolver> {
        Arc::new(self)
    }
}

impl RoleResolver for StaticRoleResolver {
    fn resolve(
        &self,
        user: &str,
        group: &str,
        platform: Option<RoleRequired>,
    ) -> Option<RoleRequired> {
        if let Some(role) = self.users.get(user) {
            return Some(*role);
        }
        if let Some(role) = self.groups.get(group) {
            return Some(*role);
        }
        platform
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FixedRoleResolver(pub Option<RoleRequired>);

impl RoleResolver for FixedRoleResolver {
    fn resolve(
        &self,
        _user: &str,
        _group: &str,
        _platform: Option<RoleRequired>,
    ) -> Option<RoleRequired> {
        self.0
    }
}

pub fn shared(resolver: impl RoleResolver + 'static) -> Arc<dyn RoleResolver> {
    Arc::new(resolver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_resolver_defaults_to_member() {
        let resolver = PlatformRoleResolver;
        assert_eq!(
            resolver.resolve("u1", "g1", None),
            Some(RoleRequired::Member)
        );
        assert_eq!(
            resolver.resolve("u1", "g1", Some(RoleRequired::Owner)),
            Some(RoleRequired::Owner)
        );
    }

    #[test]
    fn static_resolver_prefers_user_then_group_then_platform() {
        let resolver = StaticRoleResolver::new()
            .with_user("owner", RoleRequired::Owner)
            .with_group("g-admin", RoleRequired::Admin);
        assert_eq!(
            resolver.resolve("owner", "g-admin", None),
            Some(RoleRequired::Owner)
        );
        assert_eq!(
            resolver.resolve("nobody", "g-admin", None),
            Some(RoleRequired::Admin)
        );
        assert_eq!(resolver.resolve("nobody", "g-plain", None), None);
        assert_eq!(
            resolver.resolve("nobody", "g-plain", Some(RoleRequired::Member)),
            Some(RoleRequired::Member)
        );
    }

    #[test]
    fn fixed_resolver_reports_unknown() {
        let resolver = FixedRoleResolver(None);
        assert!(resolver
            .resolve("u1", "g1", Some(RoleRequired::Owner))
            .is_none());
        let resolver = FixedRoleResolver(Some(RoleRequired::Admin));
        assert_eq!(
            resolver.resolve("u1", "g1", None),
            Some(RoleRequired::Admin)
        );
    }

    #[test]
    fn shared_resolver_is_object_safe() {
        let resolver = StaticRoleResolver::new().shared();
        assert_eq!(
            resolver.resolve("u1", "g1", Some(RoleRequired::Member)),
            Some(RoleRequired::Member)
        );
        assert_eq!(
            shared(FixedRoleResolver(None)).resolve("u1", "g1", None),
            None
        );
    }
}
