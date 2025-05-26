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
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 128],
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
}

impl Seed for UserHeader {
    const SEED: &'static [u8] = b"user";
}

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
    #[cfg_attr(feature = "debug", debug(skip))]
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 64],
}

impl UserGtState {
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
}
