use anchor_lang::prelude::*;

/// Errors local to the GT incentive program.
///
/// Generic conditions (permission, overflow, value-range failures, etc.)
/// continue to use [`gmsol_store::CoreError`]. Variants here cover the
/// state-machine and precondition checks that are specific to the
/// `AirdropConfig` / `Airdrop` / `AirdropTarget` accounts.
#[error_code]
pub enum GtIncentiveError {
    /// Airdrop config has not been initialized.
    #[msg("airdrop config not initialized")]
    AirdropConfigNotInitialized,
    /// Airdrop config has already been initialized.
    #[msg("airdrop config already initialized")]
    AirdropConfigAlreadyInitialized,
    /// Airdrop has not been initialized.
    #[msg("airdrop not initialized")]
    AirdropNotInitialized,
    /// Airdrop has already been initialized.
    #[msg("airdrop already initialized")]
    AirdropAlreadyInitialized,
    /// Airdrop has not been marked complete.
    #[msg("airdrop not complete")]
    AirdropNotComplete,
    /// Airdrop has already been marked complete.
    #[msg("airdrop already complete")]
    AirdropAlreadyComplete,
    /// Airdrop has not been approved.
    #[msg("airdrop not approved")]
    AirdropNotApproved,
    /// Airdrop has already been approved.
    #[msg("airdrop already approved")]
    AirdropAlreadyApproved,
    /// Airdrop has been cancelled.
    #[msg("airdrop cancelled")]
    AirdropCancelled,
    /// Airdrop has expired.
    #[msg("airdrop expired")]
    AirdropExpired,
    /// Airdrop expiry is too close (claimable_at would exceed expiry).
    #[msg("airdrop expiry is too close")]
    AirdropExpiryTooClose,
    /// Airdrop has no targets.
    #[msg("airdrop has no targets")]
    AirdropHasNoTargets,
    /// Airdrop timelock has not elapsed.
    #[msg("airdrop timelock not elapsed")]
    AirdropTimelockNotElapsed,
    /// Airdrop target has not been initialized.
    #[msg("airdrop target not initialized")]
    AirdropTargetNotInitialized,
    /// Airdrop target has already been initialized.
    #[msg("airdrop target already initialized")]
    AirdropTargetAlreadyInitialized,
    /// Airdrop target has already been claimed.
    #[msg("airdrop target already claimed")]
    AirdropTargetAlreadyClaimed,
}
