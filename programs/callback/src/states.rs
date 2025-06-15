use anchor_lang::prelude::*;

/// The expected program ID of the caller.
pub const CALLER_PROGRAM_ID: Pubkey = pubkey!("Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo");

/// Maximum length of the prefix.
#[constant]
pub const MAX_PREFIX_LEN: u8 = 32;

/// The seed for [`Config`] account.
#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

/// The seed for [`ActionStats`] account.
#[constant]
pub const ACTION_STATS_SEED: &[u8] = b"action_stats";

/// Config account.
#[account]
#[derive(InitSpace)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Config {
    /// Prefix.
    #[max_len(MAX_PREFIX_LEN)]
    pub prefix: String,
    /// Total number of calls.
    pub calls: u64,
}

/// Account that tracks lifecycle statistics of actions known to the program.
#[account]
#[derive(InitSpace)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct ActionStats {
    /// Whether the account has been initialized.
    pub initialized: bool,
    /// Bump seed used to derive the PDA for this account.
    pub bump: u8,
    /// The action kind.
    pub action_kind: u8,
    /// The owner.
    pub owner: Pubkey,
    /// Total number of actions that have ever been created.
    pub total_created: u64,
    /// Updated times.
    pub update_count: u64,
    /// Total number of actions that have been executed.
    pub total_executed: u64,
    /// Total number of actions that have been closed.
    pub total_closed: u64,
    /// Timestamp of the last created action.
    pub last_created_at: i64,
    /// Timestamp of the last created action.
    pub last_updated_at: i64,
    /// Timestamp of the last executed action.
    pub last_executed_at: i64,
    /// Timestamp of the last closed action.
    pub last_closed_at: i64,
}
