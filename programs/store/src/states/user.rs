use anchor_lang::prelude::*;
use gmsol_utils::{
    user::{UserFlag, MAX_USER_FLAGS},
    InitSpace,
};

use crate::{
    utils::pubkey::{optional_address, DEFAULT_PUBKEY},
    CoreError,
};

use super::Seed;

/// Header of `User` Account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UserHeader {
    /// Version of the user account.
    pub(crate) version: u8,
    /// The bump seed.
    pub(crate) bump: u8,
    flags: UserFlagContainer,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 13],
    /// The owner of this user account.
    pub(crate) owner: Pubkey,
    /// The store.
    pub(crate) store: Pubkey,
    /// Referral.
    pub(crate) referral: Referral,
    /// GT State.
    pub(crate) gt: UserGtState,
    /// This user's builder fee factor, as a builder. `0` means the user
    /// has not advertised a rate.
    pub(crate) builder_fee_factor: u128,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 112],
}

gmsol_utils::flags!(UserFlag, MAX_USER_FLAGS, u8);

impl UserHeader {
    /// Return whether the user account is initialized.
    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(UserFlag::Initialized)
    }

    /// Initialize.
    pub(crate) fn init(&mut self, store: &Pubkey, owner: &Pubkey, bump: u8) -> Result<()> {
        require!(
            !self.flags.get_flag(UserFlag::Initialized),
            CoreError::UserAccountHasBeenInitialized
        );
        self.flags.set_flag(UserFlag::Initialized, true);

        self.bump = bump;
        self.owner = *owner;
        self.store = *store;

        Ok(())
    }

    /// Get User Account space.
    pub fn space(_version: u8) -> usize {
        std::mem::size_of::<Self>()
    }

    /// Get referral.
    pub fn referral(&self) -> &Referral {
        &self.referral
    }

    /// Transfer the ownership of the given code from this user to the receiver.
    /// # CHECK
    /// - `code` must be owned by current user.
    /// - the store of `code` must be the same as current user and `receiver`.
    /// # Errors
    /// - `code` must be initialized.
    /// - current user must be initialized.
    /// - `receiver` must be initialized.
    /// - the code of `receiver` must not have been set.
    /// - the `next_owner` of the code must be the owner of the `receiver_user`.
    pub(crate) fn unchecked_complete_code_transfer(
        &mut self,
        code: &mut ReferralCodeV2,
        receiver_user: &mut Self,
    ) -> Result<()> {
        require!(
            code.code != ReferralCodeBytes::default(),
            CoreError::PreconditionsAreNotMet
        );
        require!(self.is_initialized(), CoreError::InvalidUserAccount);
        require!(
            receiver_user.is_initialized(),
            CoreError::InvalidUserAccount
        );
        require_keys_eq!(
            receiver_user.referral.code,
            DEFAULT_PUBKEY,
            CoreError::PreconditionsAreNotMet
        );
        require_keys_eq!(
            receiver_user.owner,
            code.next_owner,
            CoreError::PreconditionsAreNotMet
        );

        // Transfer the ownership.
        receiver_user.referral.code = self.referral.code;
        code.owner = receiver_user.owner;
        self.referral.code = DEFAULT_PUBKEY;
        Ok(())
    }

    /// Transfer the ownership of the given code from this user to the receiver.
    /// # CHECK
    /// - `code` must be owned by current user.
    /// - the store of `code` must be the same as current user and `receiver`.
    /// # Errors
    /// - `code` must be initialized.
    /// - current user must be initialized.
    /// - `receiver_user` must be initialized.
    /// - the code of `receiver_user` must not have been set.
    pub(crate) fn unchecked_transfer_code(
        &self,
        code: &mut ReferralCodeV2,
        receiver_user: &Self,
    ) -> Result<()> {
        require!(
            code.code != ReferralCodeBytes::default(),
            CoreError::PreconditionsAreNotMet
        );
        require!(self.is_initialized(), CoreError::InvalidUserAccount);
        require!(
            receiver_user.is_initialized(),
            CoreError::InvalidUserAccount
        );
        require_keys_eq!(
            receiver_user.referral.code,
            DEFAULT_PUBKEY,
            CoreError::PreconditionsAreNotMet
        );

        code.set_next_owner(&receiver_user.owner)?;

        Ok(())
    }

    /// Get GT state.
    pub fn gt(&self) -> &UserGtState {
        &self.gt
    }

    /// Get this user's builder fee factor, as a builder.
    pub fn builder_fee_factor(&self) -> u128 {
        self.builder_fee_factor
    }

    /// Set this user's builder fee factor, as a builder, returning the previous one.
    ///
    /// The caller is responsible for validating the factor against the
    /// store-level cap: this only records the advertised rate.
    ///
    /// Upholds the authorization invariant structurally: the User Account PDA
    /// seeds bind the account to its signer, so advertising on someone else's
    /// account is unconstructible rather than merely rejected. See the builder
    /// fee invariants on [`crate::states::order`].
    pub(crate) fn set_builder_fee_factor(&mut self, factor: u128) -> u128 {
        std::mem::replace(&mut self.builder_fee_factor, factor)
    }
}

impl Seed for UserHeader {
    const SEED: &'static [u8] = b"user";
}

/// The seed of the (per-user, per-token) user token controller PDA.
///
/// This PDA has no backing account and no init instruction yet; today its
/// only purpose is to fix the derivation scheme, `[SEED, user_account,
/// token_mint]`, so future claim/set-builder-fee instructions can reserve
/// the account slot ahead of the controller having any data or behavior.
/// A future "controller account exists" check degenerates to "the passed
/// account matches this PDA derivation".
#[constant]
pub const USER_TOKEN_CONTROLLER_SEED: &[u8] = b"user_token_controller";

/// Referral Code Bytes.
pub type ReferralCodeBytes = [u8; 8];

/// Referral.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Referral {
    /// The (owner) address of the referrer.
    ///
    /// [`DEFAULT_PUBKEY`] means no referrer.
    pub(crate) referrer: Pubkey,
    /// Referral Code Address.
    pub(crate) code: Pubkey,
    /// Number of referee.
    referee_count: u128,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl Referral {
    pub(crate) fn set_code(&mut self, code: &Pubkey) -> Result<()> {
        require_keys_eq!(self.code, DEFAULT_PUBKEY, CoreError::ReferralCodeHasBeenSet);

        self.code = *code;

        Ok(())
    }

    pub(crate) fn set_referrer(&mut self, referrer_user: &mut UserHeader) -> Result<()> {
        require_keys_eq!(self.referrer, DEFAULT_PUBKEY, CoreError::ReferrerHasBeenSet,);

        require!(
            referrer_user.owner != DEFAULT_PUBKEY,
            CoreError::InvalidArgument
        );

        self.referrer = referrer_user.owner;
        referrer_user.referral.referee_count =
            referrer_user.referral.referee_count.saturating_add(1);

        Ok(())
    }

    /// Get the user account address of the referrer.
    pub fn referrer(&self) -> Option<&Pubkey> {
        optional_address(&self.referrer)
    }

    /// Get the referral code account address.
    pub fn code(&self) -> Option<&Pubkey> {
        optional_address(&self.code)
    }
}

/// Referral Code.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReferralCodeV2 {
    version: u8,
    /// Bump.
    pub(crate) bump: u8,
    /// Code bytes.
    pub code: ReferralCodeBytes,
    /// Store.
    pub store: Pubkey,
    /// Owner.
    pub owner: Pubkey,
    /// Next owner.
    next_owner: Pubkey,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl ReferralCodeV2 {
    /// The length of referral code.
    pub const LEN: usize = std::mem::size_of::<ReferralCodeBytes>();

    pub(crate) fn init(
        &mut self,
        bump: u8,
        code: ReferralCodeBytes,
        store: &Pubkey,
        owner: &Pubkey,
    ) {
        self.bump = bump;
        self.code = code;
        self.store = *store;
        self.owner = *owner;
        self.next_owner = *owner;
    }

    /// Get next owner.
    pub fn next_owner(&self) -> &Pubkey {
        &self.next_owner
    }

    pub(crate) fn set_next_owner(&mut self, next_owner: &Pubkey) -> Result<()> {
        require_keys_neq!(
            self.next_owner,
            *next_owner,
            CoreError::PreconditionsAreNotMet
        );
        self.next_owner = *next_owner;
        Ok(())
    }

    #[cfg(feature = "utils")]
    /// Decode the given code string to code bytes.
    pub fn decode(code: &str) -> Result<ReferralCodeBytes> {
        require!(!code.is_empty(), CoreError::InvalidArgument);
        let code = bs58::decode(code)
            .into_vec()
            .map_err(|_| error!(CoreError::InvalidArgument))?;
        require_gte!(Self::LEN, code.len(), CoreError::InvalidArgument);
        let padding = Self::LEN - code.len();
        let mut code_bytes = ReferralCodeBytes::default();
        code_bytes[padding..].copy_from_slice(&code);

        Ok(code_bytes)
    }

    #[cfg(feature = "utils")]
    /// Encode the given code to code string.
    pub fn encode(code: &ReferralCodeBytes, skip_leading_ones: bool) -> String {
        let code = bs58::encode(code).into_string();
        if skip_leading_ones {
            code.trim_start_matches('1').to_owned()
        } else {
            code
        }
    }
}

impl InitSpace for ReferralCodeV2 {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for ReferralCodeV2 {
    const SEED: &'static [u8] = b"referral_code";
}

/// GT State.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UserGtState {
    pub(crate) rank: u8,
    padding_0: [u8; 7],
    pub(crate) last_minted_at: i64,
    pub(crate) total_minted: u64,
    pub(crate) amount: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_1: [u8; 32],
    pub(crate) paid_fee_value: u128,
    pub(crate) minted_fee_value: u128,
    /// The address authorized to spend GT on behalf of this user.
    /// [`DEFAULT_PUBKEY`] (the zero address) means no delegate is set.
    delegate: Pubkey,
    /// The remaining GT amount the delegate may spend, in raw units.
    allowance: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 24],
}

impl UserGtState {
    /// Replace the delegate and its remaining allowance together.
    pub(crate) fn set_delegate(&mut self, delegate: Pubkey, allowance: u64) -> Result<()> {
        require!(
            optional_address(&delegate).is_some() || allowance == 0,
            CoreError::InvalidArgument
        );
        self.delegate = delegate;
        self.allowance = allowance;
        Ok(())
    }

    /// Add to the allowance that remains when this operation executes.
    pub(crate) fn increase_allowance(&mut self, delegate: &Pubkey, amount: u64) -> Result<()> {
        require!(amount > 0, CoreError::InvalidArgument);
        require_keys_eq!(
            *self.delegate().ok_or(CoreError::InvalidArgument)?,
            *delegate,
            CoreError::InvalidArgument
        );
        self.allowance = self
            .allowance
            .checked_add(amount)
            .ok_or(CoreError::InvalidArgument)?;
        Ok(())
    }

    /// Reduce the current allowance, stopping at zero.
    pub(crate) fn decrease_allowance(&mut self, delegate: &Pubkey, amount: u64) -> Result<()> {
        require!(amount > 0, CoreError::InvalidArgument);
        require_keys_eq!(
            *self.delegate().ok_or(CoreError::InvalidArgument)?,
            *delegate,
            CoreError::InvalidArgument
        );
        self.allowance = self.allowance.saturating_sub(amount);
        Ok(())
    }

    /// Get total paid fee value.
    pub fn paid_fee_value(&self) -> u128 {
        self.paid_fee_value
    }

    /// Get minted fee value.
    pub fn minted_fee_value(&self) -> u128 {
        self.minted_fee_value
    }

    /// Get current rank.
    pub fn rank(&self) -> u8 {
        self.rank
    }

    /// Get GT balance.
    pub fn amount(&self) -> u64 {
        self.amount
    }

    /// Get the GT delegate, if one is set.
    pub fn delegate(&self) -> Option<&Pubkey> {
        optional_address(&self.delegate)
    }

    /// Get the remaining GT allowance in raw units.
    pub fn allowance(&self) -> u64 {
        self.allowance
    }

    pub(crate) fn spend_allowance(&mut self, delegate: &Pubkey, amount: u64) -> Result<()> {
        require!(
            self.delegate() == Some(delegate),
            CoreError::PermissionDenied
        );
        self.allowance = self
            .allowance
            .checked_sub(amount)
            .ok_or(CoreError::InvalidArgument)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The `UserHeader` account's byte layout. These must never change.
    const EXPECTED_USER_HEADER_ACCOUNT_SIZE: usize = 512;
    const GT_OFFSET: usize = 224;
    const GT_DELEGATE_OFFSET: usize = 320;
    const GT_ALLOWANCE_OFFSET: usize = 352;
    const BUILDER_FEE_FACTOR_OFFSET: usize = 384;

    #[test]
    fn user_header_account_layout() {
        assert_eq!(
            std::mem::size_of::<UserHeader>(),
            EXPECTED_USER_HEADER_ACCOUNT_SIZE
        );
        assert_eq!(std::mem::offset_of!(UserHeader, gt), GT_OFFSET);
        assert_eq!(
            GT_OFFSET + std::mem::offset_of!(UserGtState, delegate),
            GT_DELEGATE_OFFSET
        );
        assert_eq!(
            GT_OFFSET + std::mem::offset_of!(UserGtState, allowance),
            GT_ALLOWANCE_OFFSET
        );
        assert_eq!(
            std::mem::offset_of!(UserHeader, builder_fee_factor),
            BUILDER_FEE_FACTOR_OFFSET
        );
        // u128 requires 16-byte alignment.
        assert_eq!(BUILDER_FEE_FACTOR_OFFSET % 16, 0);
    }

    #[test]
    fn existing_zeroed_accounts_read_as_no_builder_fee_factor() {
        // Every existing on-chain `UserHeader` account has zero bytes
        // throughout what used to be its `reserved` region.
        let user: UserHeader = bytemuck::Zeroable::zeroed();
        assert_eq!(user.builder_fee_factor(), 0);
    }

    #[test]
    fn zeroed_user_accounts_have_no_gt_delegate_or_allowance() {
        let user: UserHeader = bytemuck::Zeroable::zeroed();
        assert_eq!(user.gt().delegate(), None);
        assert_eq!(user.gt().allowance(), 0);
    }

    #[test]
    fn gt_allowance_accepts_full_supply_range() {
        let mut user: UserHeader = bytemuck::Zeroable::zeroed();
        user.gt.allowance = u64::MAX;
        assert_eq!(user.gt().allowance(), u64::MAX);
    }

    #[test]
    fn scub_gt_delegation_replaces_allowance_and_clears_it() {
        let mut user: UserHeader = bytemuck::Zeroable::zeroed();
        let scub_first = Pubkey::new_unique();
        let scub_second = Pubkey::new_unique();
        user.gt.set_delegate(scub_first, 100).unwrap();
        user.gt.decrease_allowance(&scub_first, 25).unwrap();
        user.gt.set_delegate(scub_second, 30).unwrap();
        assert_eq!(user.gt.delegate(), Some(&scub_second));
        assert_eq!(user.gt.allowance(), 30);
        user.gt.set_delegate(scub_second, 12).unwrap();
        assert_eq!(user.gt.allowance(), 12);
        user.gt.set_delegate(DEFAULT_PUBKEY, 0).unwrap();
        assert_eq!(user.gt.delegate(), None);
        assert_eq!(user.gt.allowance(), 0);
        assert!(user.gt.set_delegate(DEFAULT_PUBKEY, 1).is_err());
        assert_eq!(user.gt.allowance(), 0);
    }

    #[test]
    fn scub_gt_allowance_changes_use_current_remaining_amount() {
        let mut user: UserHeader = bytemuck::Zeroable::zeroed();
        let scub_delegate = Pubkey::new_unique();
        let scub_other = Pubkey::new_unique();
        assert!(user.gt.increase_allowance(&scub_delegate, 1).is_err());
        assert!(user.gt.decrease_allowance(&scub_delegate, 1).is_err());
        assert_eq!(user.gt.allowance(), 0);
        user.gt.set_delegate(scub_delegate, 100).unwrap();
        assert!(user.gt.increase_allowance(&scub_other, 1).is_err());
        assert!(user.gt.decrease_allowance(&scub_other, 1).is_err());
        assert!(user.gt.increase_allowance(&scub_delegate, 0).is_err());
        assert!(user.gt.decrease_allowance(&scub_delegate, 0).is_err());
        user.gt.decrease_allowance(&scub_delegate, 40).unwrap(); // An intervening spend.
        user.gt.increase_allowance(&scub_delegate, 15).unwrap();
        assert_eq!(user.gt.allowance(), 75);
        user.gt.decrease_allowance(&scub_delegate, 100).unwrap();
        assert_eq!(user.gt.allowance(), 0);
        user.gt.set_delegate(scub_other, u64::MAX).unwrap();
        assert!(user.gt.increase_allowance(&scub_delegate, 1).is_err());
        assert!(user.gt.increase_allowance(&scub_other, 1).is_err());
        assert_eq!(user.gt.allowance(), u64::MAX);
    }

    #[test]
    fn gt_delegate_spend_requires_current_delegate_and_remaining_allowance() {
        let mut user: UserHeader = bytemuck::Zeroable::zeroed();
        let delegate = Pubkey::new_unique();
        let unauthorized = Pubkey::new_unique();
        user.gt.set_delegate(delegate, 10).unwrap();
        assert!(user.gt.spend_allowance(&unauthorized, 1).is_err());
        assert_eq!(user.gt.allowance(), 10);
        assert!(user.gt.spend_allowance(&delegate, 11).is_err());
        assert_eq!(user.gt.allowance(), 10);
        user.gt.spend_allowance(&delegate, 7).unwrap();
        assert_eq!(user.gt.allowance(), 3);
        user.gt.set_delegate(unauthorized, 2).unwrap();
        assert!(user.gt.spend_allowance(&delegate, 1).is_err());
        assert_eq!(user.gt.allowance(), 2);
    }
}
