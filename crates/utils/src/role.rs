use anchor_lang::prelude::*;
use std::borrow::Borrow;

/// Max length of the role anme.
pub const MAX_ROLE_NAME_LEN: usize = 32;

/// The key of a Role.
#[derive(Clone, AnchorSerialize, AnchorDeserialize, InitSpace, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct RoleKey {
    #[max_len(MAX_ROLE_NAME_LEN)]
    name: String,
}

impl RoleKey {
    /// Oracle Controller.
    pub const ORACLE_CONTROLLER: &'static str = "ORACLE_CONTROLLER";

    /// GT Controller.
    pub const GT_CONTROLLER: &'static str = "GT_CONTROLLER";

    /// Market Keeper.
    pub const MARKET_KEEPER: &'static str = "MARKET_KEEPER";

    /// Order Keeper.
    pub const ORDER_KEEPER: &'static str = "ORDER_KEEPER";

    /// Feature Keeper.
    pub const FEATURE_KEEPER: &'static str = "FEATURE_KEEPER";

    /// Config Keeper.
    pub const CONFIG_KEEPER: &'static str = "CONFIG_KEEPER";

    /// Restart Admin.
    /// When the cluster restarts, this role can be used for **any** role, including ADMIN."
    pub const RESTART_ADMIN: &'static str = "RESTART_ADMIN";

    /// Price Keeper.
    pub const PRICE_KEEPER: &'static str = "PRICE_KEEPER";

    /// Migration Keeper.
    pub const MIGRATION_KEEPER: &'static str = "MIGRATION_KEEPER";
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
