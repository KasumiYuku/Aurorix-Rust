use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoleRequired {
    Owner,
    Admin,
    Member,
}

impl RoleRequired {
    pub const fn as_str(self) -> &'static str {
        match self {
            RoleRequired::Owner => "owner",
            RoleRequired::Admin => "admin",
            RoleRequired::Member => "member",
        }
    }

    pub const fn can_use(self, user: Option<RoleRequired>) -> bool {
        let Some(user) = user else {
            return false;
        };
        match self {
            RoleRequired::Owner => matches!(user, RoleRequired::Owner),
            RoleRequired::Admin => matches!(user, RoleRequired::Admin | RoleRequired::Owner),
            RoleRequired::Member => true,
        }
    }
}

impl fmt::Display for RoleRequired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RoleRequired {
    type Err = CoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "" => Ok(RoleRequired::Member),
            "owner" => Ok(RoleRequired::Owner),
            "admin" => Ok(RoleRequired::Admin),
            "member" => Ok(RoleRequired::Member),
            other => Err(CoreError::Invalid(format!("未知的角色: {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_requires_owner() {
        assert!(RoleRequired::Owner.can_use(Some(RoleRequired::Owner)));
        assert!(!RoleRequired::Owner.can_use(Some(RoleRequired::Admin)));
        assert!(!RoleRequired::Owner.can_use(Some(RoleRequired::Member)));
    }

    #[test]
    fn admin_accepts_admin_and_owner() {
        assert!(RoleRequired::Admin.can_use(Some(RoleRequired::Owner)));
        assert!(RoleRequired::Admin.can_use(Some(RoleRequired::Admin)));
        assert!(!RoleRequired::Admin.can_use(Some(RoleRequired::Member)));
    }

    #[test]
    fn member_accepts_any_known_role() {
        assert!(RoleRequired::Member.can_use(Some(RoleRequired::Member)));
        assert!(RoleRequired::Member.can_use(Some(RoleRequired::Admin)));
    }

    #[test]
    fn unknown_user_is_denied() {
        assert!(!RoleRequired::Member.can_use(None));
        assert!(!RoleRequired::Admin.can_use(None));
    }

    #[test]
    fn empty_role_parses_to_member() {
        assert_eq!("".parse::<RoleRequired>().unwrap(), RoleRequired::Member);
        assert_eq!(
            "admin".parse::<RoleRequired>().unwrap(),
            RoleRequired::Admin
        );
        assert!("root".parse::<RoleRequired>().is_err());
    }
}
