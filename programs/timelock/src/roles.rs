/// Timelock Admin.
pub const TIMELOCK_ADMIN: &str = "TIMELOCK_ADMIN";

/// Timelock Keeper.
pub const TIMELOCK_KEEPER: &str = "TIMELOCK_KEEPER";

/// Timelocked prefix.
pub const TIMELOCKED: &str = "__TLD_";

/// Admin role name.
pub const ADMIN: &str = "ADMIN";

/// Timelocked admin.
pub const TIMELOCKED_ADMIN: &str = "__TLD_ADMIN";

/// Timelocked market keeper.
pub const TIMELOCKED_MARKET_KEEPER: &str = "__TLD_MARKET_KEEPER";

/// Get timelocked role.
pub fn timelocked_role(role: &str) -> String {
    [TIMELOCKED, role].concat()
}

#[cfg(test)]
mod tests {
    use gmsol_store::states::RoleKey;

    use super::*;

    #[test]
    fn validate_timelocked_admin() {
        assert_eq!(TIMELOCKED_ADMIN, timelocked_role(ADMIN));
    }

    #[test]
    fn validate_timelocked_market_keeper() {
        assert_eq!(
            TIMELOCKED_MARKET_KEEPER,
            timelocked_role(RoleKey::MARKET_KEEPER)
        );
    }
}
