use anchor_lang::prelude::*;

use crate::states::user::ReferralCodeBytes;

/// Referral Code.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct ReferralCode {
    /// Bump.
    pub(crate) bump: u8,
    /// Code bytes.
    pub code: ReferralCodeBytes,
    /// Store.
    pub store: Pubkey,
    /// Owner.
    pub owner: Pubkey,
}
