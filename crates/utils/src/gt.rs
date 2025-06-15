/// Max number of GT exchange vault flags.
pub const MAX_GT_EXCHANGE_VAULT_FLAGS: usize = 8;

/// Max number of GT exchange flags.
pub const MAX_GT_EXCHANGE_FLAGS: usize = 8;

/// Max number of GT bank flags.
pub const MAX_GT_BANK_FLAGS: usize = 8;

/// Get time window index.
pub fn get_time_window_index(ts: i64, time_window: i64) -> i64 {
    debug_assert!(time_window > 0);
    ts / time_window
}

/// GT Exchange Vault Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum GtExchangeVaultFlag {
    /// Initialized.
    Initialized,
    /// Confirmed.
    Confirmed,
    // CHECK: should have no more than `MAX_GT_EXCHANGE_VAULT_FLAGS` of flags.
}

/// GT Exchange Vault Flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
pub enum GtExchangeFlag {
    /// Initialized.
    Initialized,
    // CHECK: should have no more than `MAX_GT_EXCHANGE_FLAGS` of flags.
}

/// Flags of GT Bank.
#[derive(num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum GtBankFlags {
    /// Initialized.
    Initialized,
    /// Confirmed.
    Confirmed,
    /// Synced after confirmation.
    SyncedAfterConfirmation,
    // CHECK: cannot have more than `MAX_GT_BANK_FLAGS` flags.
}
