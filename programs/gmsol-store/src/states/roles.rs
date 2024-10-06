use std::borrow::Borrow;

use anchor_lang::prelude::*;
use bitmaps::Bitmap;

use crate::StoreError;

use super::InitSpace;

/// Max length of the role anme.
pub const MAX_ROLE_NAME_LEN: usize = 32;

/// Max number of roles.
pub const MAX_ROLES: usize = 32;

/// Max number of members.
pub const MAX_MEMBERS: usize = 16;

/// The key of a Role.
#[derive(Clone, AnchorSerialize, AnchorDeserialize, InitSpace, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct RoleKey {
    #[max_len(MAX_ROLE_NAME_LEN)]
    name: String,
}

impl RoleKey {
    /// CONTROLLER.
    pub const CONTROLLER: &'static str = "CONTROLLER";

    /// MARKET KEEPER.
    pub const MARKET_KEEPER: &'static str = "MARKET_KEEPER";

    /// ORDER KEEPER.
    pub const ORDER_KEEPER: &'static str = "ORDER_KEEPER";

    /// FEATURE KEEPER.
    pub const FEATURE_KEEPER: &'static str = "FEATURE_KEEPER";
}

impl Borrow<str> for RoleKey {
    fn borrow(&self) -> &str {
        &self.name
    }
}

impl<'a> From<&'a str> for RoleKey {
    fn from(value: &'a str) -> Self {
        Self {
            name: value.to_owned(),
        }
    }
}

type RoleBitmap = Bitmap<MAX_ROLES>;
type RoleBitmapValue = u32;

/// Role Metadata.
#[zero_copy]
#[derive(Default)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct RoleMetadata {
    name: [u8; MAX_ROLE_NAME_LEN],
    enabled: u8,
    index: u8,
}

impl InitSpace for RoleMetadata {
    const INIT_SPACE: usize = 32 + 2;
}

#[cfg(test)]
const_assert_eq!(
    std::mem::size_of::<RoleMetadata>(),
    RoleMetadata::INIT_SPACE
);

impl RoleMetadata {
    /// A `u8` value indicates that this role is enabled.
    pub const ROLE_ENABLED: u8 = u8::MAX;

    fn name_to_bytes(name: &str) -> Result<[u8; MAX_ROLE_NAME_LEN]> {
        crate::utils::fixed_str::fixed_str_to_bytes(name)
    }

    fn bytes_to_name(bytes: &[u8; 32]) -> Result<&str> {
        crate::utils::fixed_str::bytes_to_fixed_str(bytes)
    }

    /// Create a new role metadata.
    pub fn new(name: &str, index: u8) -> Result<Self> {
        Ok(Self {
            name: Self::name_to_bytes(name)?,
            enabled: Self::ROLE_ENABLED,
            index,
        })
    }

    /// Get the name of this role.
    pub fn name(&self) -> Result<&str> {
        Self::bytes_to_name(&self.name)
    }

    /// Enable this role.
    pub fn enable(&mut self) {
        self.enabled = Self::ROLE_ENABLED;
    }

    /// Disable this role.
    pub fn disable(&mut self) {
        self.enabled = 0;
    }

    /// Is enbaled.
    pub fn is_enabled(&self) -> bool {
        self.enabled == Self::ROLE_ENABLED
    }
}

gmsol_utils::fixed_map!(RoleMap, RoleMetadata, MAX_ROLES, 0);

gmsol_utils::fixed_map!(
    Members,
    Pubkey,
    crate::utils::pubkey::to_bytes,
    u32,
    MAX_MEMBERS,
    0
);

/// Roles Store.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct RoleStore {
    roles: RoleMap,
    members: Members,
}

impl InitSpace for RoleStore {
    const INIT_SPACE: usize = std::mem::size_of::<RoleStore>();
}

impl RoleStore {
    /// Enable a role.
    pub fn enable_role(&mut self, role: &str) -> Result<()> {
        match self.roles.get_mut(role) {
            Some(metadata) => {
                require_eq!(metadata.name()?, role, StoreError::InvalidArgument);
                metadata.enable();
            }
            None => {
                let index = self
                    .roles
                    .len()
                    .try_into()
                    .map_err(|_| error!(StoreError::ExceedMaxLengthLimit))?;
                self.roles
                    .insert_with_options(role, RoleMetadata::new(role, index)?, true)?;
            }
        }
        Ok(())
    }

    /// Disable a role.
    pub fn disable_role(&mut self, role: &str) -> Result<()> {
        if let Some(metadata) = self.roles.get_mut(role) {
            require_eq!(metadata.name()?, role, StoreError::InvalidArgument);
            metadata.disable();
        }
        Ok(())
    }

    /// Get the index of a enabled role.
    pub fn enabled_role_index(&self, role: &str) -> Result<Option<u8>> {
        if let Some(metadata) = self.roles.get(role) {
            require_eq!(metadata.name()?, role, StoreError::InvalidArgument);
            require!(metadata.is_enabled(), StoreError::DisabledRole);
            Ok(Some(metadata.index))
        } else {
            Ok(None)
        }
    }

    /// Check if the given role is granted to the pubkey.
    pub fn has_role(&self, authority: &Pubkey, role: &str) -> Result<bool> {
        let Some(value) = self.members.get(authority) else {
            return err!(StoreError::PermissionDenied);
        };
        let Some(index) = self.enabled_role_index(role)? else {
            return err!(StoreError::NoSuchRole);
        };
        let bitmap = RoleBitmap::from_value(*value);
        Ok(bitmap.get(index as usize))
    }

    /// Grant a role to the pubkey.
    pub fn grant(&mut self, authority: &Pubkey, role: &str) -> Result<()> {
        let Some(index) = self.enabled_role_index(role)? else {
            return err!(StoreError::NoSuchRole);
        };
        match self.members.get_mut(authority) {
            Some(value) => {
                let mut bitmap = RoleBitmap::from_value(*value);
                bitmap.set(index as usize, true);
                *value = bitmap.into_value();
            }
            None => {
                let mut bitmap = RoleBitmap::new();
                bitmap.set(index as usize, true);
                self.members
                    .insert_with_options(authority, bitmap.into_value(), true)?;
            }
        }
        Ok(())
    }

    /// Revoke a role from the pubkey.
    pub fn revoke(&mut self, authority: &Pubkey, role: &str) -> Result<()> {
        let Some(index) = self.enabled_role_index(role)? else {
            return err!(StoreError::NoSuchRole);
        };
        let Some(value) = self.members.get_mut(authority) else {
            return err!(StoreError::PermissionDenied);
        };
        let mut bitmap = RoleBitmap::from_value(*value);
        bitmap.set(index as usize, false);
        *value = bitmap.into_value();
        Ok(())
    }

    /// Get the number of roles.
    pub fn num_roles(&self) -> usize {
        self.roles.len()
    }

    /// Get the number of members.
    pub fn num_members(&self) -> usize {
        self.members.len()
    }

    /// Get role value for the user.
    pub fn role_value(&self, user: &Pubkey) -> Option<RoleBitmapValue> {
        self.members.get(user).copied()
    }

    /// Get all members.
    pub fn members(&self) -> impl Iterator<Item = Pubkey> + '_ {
        self.members
            .entries()
            .map(|(key, _)| Pubkey::new_from_array(*key))
    }

    /// Get all roles.
    pub fn roles(&self) -> impl Iterator<Item = Result<&str>> + '_ {
        self.roles.entries().map(|(_, value)| value.name())
    }
}

#[cfg(test)]
mod tests {
    use bytemuck::Zeroable;

    use super::*;

    #[test]
    fn grant_and_revoke_roles() {
        let mut store = RoleStore::zeroed();
        let authority = Pubkey::new_unique();

        assert!(store.grant(&authority, RoleKey::CONTROLLER).is_err());
        assert!(store.has_role(&authority, RoleKey::CONTROLLER).is_err());

        store.enable_role(RoleKey::CONTROLLER).unwrap();
        store.enable_role(RoleKey::MARKET_KEEPER).unwrap();

        store.grant(&authority, RoleKey::CONTROLLER).unwrap();
        assert_eq!(store.has_role(&authority, RoleKey::CONTROLLER), Ok(true));
        store.grant(&authority, RoleKey::MARKET_KEEPER).unwrap();
        assert_eq!(store.has_role(&authority, RoleKey::MARKET_KEEPER), Ok(true));
        assert_eq!(store.has_role(&authority, RoleKey::CONTROLLER), Ok(true));

        store.revoke(&authority, RoleKey::CONTROLLER).unwrap();
        assert_eq!(store.has_role(&authority, RoleKey::MARKET_KEEPER), Ok(true));
        assert_eq!(store.has_role(&authority, RoleKey::CONTROLLER), Ok(false));

        store.revoke(&authority, RoleKey::MARKET_KEEPER).unwrap();
        assert_eq!(
            store.has_role(&authority, RoleKey::MARKET_KEEPER),
            Ok(false)
        );
        assert_eq!(store.has_role(&authority, RoleKey::CONTROLLER), Ok(false));

        store.disable_role(RoleKey::MARKET_KEEPER).unwrap();
        assert!(store.grant(&authority, RoleKey::MARKET_KEEPER).is_err());
        assert!(store.has_role(&authority, RoleKey::MARKET_KEEPER).is_err());
        store.enable_role(RoleKey::MARKET_KEEPER).unwrap();
        store.grant(&authority, RoleKey::MARKET_KEEPER).unwrap();
        assert_eq!(store.has_role(&authority, RoleKey::MARKET_KEEPER), Ok(true));
    }

    #[test]
    fn enable_and_disable_role() {
        let mut store = RoleStore::zeroed();
        let authority = Pubkey::new_unique();

        store.enable_role(RoleKey::CONTROLLER).unwrap();
        store.grant(&authority, RoleKey::CONTROLLER).unwrap();
        assert_eq!(store.has_role(&authority, RoleKey::CONTROLLER), Ok(true));
        store.disable_role(RoleKey::CONTROLLER).unwrap();
        assert!(store.has_role(&authority, RoleKey::CONTROLLER).is_err());
        store.enable_role(RoleKey::CONTROLLER).unwrap();
        assert_eq!(store.has_role(&authority, RoleKey::CONTROLLER), Ok(true));
    }
}
