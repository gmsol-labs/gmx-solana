/// Max number of airdrop config flags.
pub const MAX_AIRDROP_CONFIG_FLAGS: usize = 8;

/// Max number of airdrop flags.
pub const MAX_AIRDROP_FLAGS: usize = 8;

/// Max number of airdrop target flags.
pub const MAX_AIRDROP_TARGET_FLAGS: usize = 8;

/// Airdrop config flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive)]
pub enum AirdropConfigFlag {
    /// Initialized.
    Initialized,
    // CHECK: should have no more than `MAX_AIRDROP_CONFIG_FLAGS` of flags.
}

/// Airdrop flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive)]
pub enum AirdropFlag {
    /// Initialized.
    Initialized,
    /// Airdrop target list has been marked as complete.
    Complete,
    /// Airdrop has been approved by Gov.
    Approved,
    /// Airdrop has been cancelled.
    Cancelled,
    // CHECK: should have no more than `MAX_AIRDROP_FLAGS` of flags.
}

/// Airdrop target flags.
#[repr(u8)]
#[non_exhaustive]
#[derive(num_enum::IntoPrimitive)]
pub enum AirdropTargetFlag {
    /// Initialized.
    Initialized,
    /// GT has been claimed by the recipient.
    Claimed,
    // CHECK: should have no more than `MAX_AIRDROP_TARGET_FLAGS` of flags.
}
