use anchor_lang::prelude::*;

/// Emitted when an admin adds or updates an operator in the [`AirdropConfig`].
#[event]
pub struct AirdropOperatorUpdated {
    pub ts: i64,
    pub store: Pubkey,
    pub authority: Pubkey,
    pub operator: Pubkey,
    pub timelock_secs: u64,
    pub max_airdrop_amount: u64,
    pub is_enabled: bool,
}

impl AirdropOperatorUpdated {
    pub(crate) fn new(
        store: Pubkey,
        authority: Pubkey,
        operator: Pubkey,
        timelock_secs: u64,
        max_airdrop_amount: u64,
        is_enabled: bool,
    ) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            store,
            authority,
            operator,
            timelock_secs,
            max_airdrop_amount,
            is_enabled,
        })
    }
}

/// Emitted when an operator creates a new airdrop campaign (S1.1).
#[event]
pub struct AirdropCreated {
    pub ts: i64,
    pub store: Pubkey,
    pub airdrop: Pubkey,
    pub operator: Pubkey,
    pub nonce: [u8; 8],
    pub expiry: i64,
}

impl AirdropCreated {
    pub(crate) fn new(
        store: Pubkey,
        airdrop: Pubkey,
        operator: Pubkey,
        nonce: [u8; 8],
        expiry: i64,
    ) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            store,
            airdrop,
            operator,
            nonce,
            expiry,
        })
    }
}

/// Emitted when an operator adds a recipient to an airdrop's target list (S1.2).
#[event]
pub struct AirdropTargetAdded {
    pub ts: i64,
    pub airdrop: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
}

impl AirdropTargetAdded {
    pub(crate) fn new(airdrop: Pubkey, recipient: Pubkey, amount: u64) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            airdrop,
            recipient,
            amount,
        })
    }
}

/// Emitted when an operator marks an airdrop's target list complete (S1.3).
#[event]
pub struct AirdropCompleted {
    pub ts: i64,
    pub airdrop: Pubkey,
    pub total_amount: u64,
    pub target_count: u64,
}

impl AirdropCompleted {
    pub(crate) fn new(airdrop: Pubkey, total_amount: u64, target_count: u64) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            airdrop,
            total_amount,
            target_count,
        })
    }
}

/// Emitted when gov approves an airdrop and the timelock starts (S1.4).
#[event]
pub struct AirdropApproved {
    pub ts: i64,
    pub airdrop: Pubkey,
    pub authority: Pubkey,
    pub claimable_at: i64,
}

impl AirdropApproved {
    pub(crate) fn new(airdrop: Pubkey, authority: Pubkey, claimable_at: i64) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            airdrop,
            authority,
            claimable_at,
        })
    }
}

/// Emitted when a recipient claims their GT from an approved airdrop (S1.5).
#[event]
pub struct AirdropClaimed {
    pub ts: i64,
    pub airdrop: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
}

impl AirdropClaimed {
    pub(crate) fn new(airdrop: Pubkey, recipient: Pubkey, amount: u64) -> Result<Self> {
        Ok(Self {
            ts: Clock::get()?.unix_timestamp,
            airdrop,
            recipient,
            amount,
        })
    }
}
