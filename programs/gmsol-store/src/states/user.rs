use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

use crate::CoreError;

use super::Seed;

/// Header of `User` Account.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct UserHeader {
    /// Version of the user account.
    pub(crate) version: u8,
    /// The bump seed.
    pub(crate) bump: u8,
    flags: UserFlagValue,
    padding_0: [u8; 13],
    /// The owner of this user account.
    pub(crate) owner: Pubkey,
    /// The store.
    pub(crate) store: Pubkey,
    /// Referral.
    pub(crate) referral: Referral,
    /// GT State.
    pub(crate) gt: GtState,
    reserved: [u8; 128],
}

impl UserHeader {
    /// Get flag.
    fn flag(&self, flag: UserFlag) -> bool {
        let map = UserFlagMap::from_value(self.flags);
        map.get(flag as usize)
    }

    /// Set flag.
    /// Return the previous value.
    fn set_flag(&mut self, flag: UserFlag, value: bool) -> bool {
        let mut map = UserFlagMap::from_value(self.flags);
        let previous = map.set(flag as usize, value);
        self.flags = map.into_value();
        previous
    }

    /// Return whether the user account is initialized.
    pub fn is_initialized(&self) -> bool {
        self.flag(UserFlag::Initialized)
    }

    /// Initialize.
    pub(crate) fn init(&mut self, store: &Pubkey, owner: &Pubkey, bump: u8) -> Result<()> {
        require!(
            !self.flag(UserFlag::Initialized),
            CoreError::UserAccountHasBeenInitialized
        );
        self.set_flag(UserFlag::Initialized, true);

        self.bump = bump;
        self.owner = *owner;
        self.store = *store;

        Ok(())
    }

    /// Get User Account space.
    pub fn space(_version: u8) -> usize {
        core::mem::size_of::<Self>()
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
    pub(crate) fn unchecked_transfer_code(
        &mut self,
        code: &mut ReferralCode,
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
        require_eq!(
            receiver_user.referral.code,
            Pubkey::default(),
            CoreError::PreconditionsAreNotMet
        );

        // Transfer the ownership.
        receiver_user.referral.code = self.referral.code;
        code.owner = receiver_user.owner;
        self.referral.code = Pubkey::default();
        Ok(())
    }

    /// Get GT state.
    pub fn gt(&self) -> &GtState {
        &self.gt
    }
}

impl Seed for UserHeader {
    const SEED: &'static [u8] = b"user";
}

/// User flags.
#[repr(u8)]
#[non_exhaustive]
pub enum UserFlag {
    /// Is initialized.
    Initialized,
}

impl UserFlag {
    /// Max flags.
    pub const MAX_FLAGS: usize = 8;
}

type UserFlagMap = bitmaps::Bitmap<{ UserFlag::MAX_FLAGS }>;
type UserFlagValue = u8;

/// Referral Code Bytes.
pub type ReferralCodeBytes = [u8; 8];

/// Referral.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Referral {
    /// The (owner) address of the referrer.
    ///
    /// `Pubkey::default()` means no referrer.
    pub(crate) referrer: Pubkey,
    /// Referral Code Address.
    pub(crate) code: Pubkey,
    /// Number of referee.
    referee_count: u128,
    reserved: [u8; 64],
}

impl Referral {
    pub(crate) fn set_code(&mut self, code: &Pubkey) -> Result<()> {
        require_eq!(
            self.code,
            Pubkey::default(),
            CoreError::ReferralCodeHasBeenSet
        );

        self.code = *code;

        Ok(())
    }

    pub(crate) fn set_referrer(&mut self, referrer_user: &mut UserHeader) -> Result<()> {
        require_eq!(
            self.referrer,
            Pubkey::default(),
            CoreError::ReferrerHasBeenSet,
        );

        require!(
            referrer_user.owner != Pubkey::default(),
            CoreError::InvalidArgument
        );

        self.referrer = referrer_user.owner;
        referrer_user.referral.referee_count =
            referrer_user.referral.referee_count.saturating_add(1);

        Ok(())
    }

    /// Get the user account address of the referrer.
    pub fn referrer(&self) -> Option<&Pubkey> {
        if self.referrer == Pubkey::default() {
            None
        } else {
            Some(&self.referrer)
        }
    }

    /// Get the referral code account address.
    pub fn code(&self) -> Option<&Pubkey> {
        if self.code == Pubkey::default() {
            None
        } else {
            Some(&self.code)
        }
    }
}

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

impl ReferralCode {
    /// The length of referral code.
    pub const LEN: usize = core::mem::size_of::<ReferralCodeBytes>();

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

impl InitSpace for ReferralCode {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Seed for ReferralCode {
    const SEED: &'static [u8] = b"referral_code";
}

/// GT State.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct GtState {
    pub(crate) rank: u8,
    padding_0: [u8; 7],
    pub(crate) last_minted_at: i64,
    pub(crate) total_minted: u64,
    pub(crate) amount: u64,
    pub(crate) es_amount: u64,
    pub(crate) vesting_es_amount: u64,
    pub(crate) es_factor: u128,
    pub(crate) traded_value: u128,
    pub(crate) minted_value: u128,
    reserved: [u8; 64],
}

impl GtState {
    /// Get traded value.
    pub fn traded_value(&self) -> u128 {
        self.traded_value
    }

    /// Get minted value.
    pub fn minted_value(&self) -> u128 {
        self.minted_value
    }

    /// Get current rank.
    pub fn rank(&self) -> u8 {
        self.rank
    }

    /// Get GT balance.
    pub fn amount(&self) -> u64 {
        self.amount
    }

    /// Get esGT balance.
    pub fn es_amount(&self) -> u64 {
        self.es_amount
    }

    /// Get vesting esGT amount.
    pub fn vesting_es_amount(&self) -> u64 {
        self.vesting_es_amount
    }

    /// Get current vaule of es factor of this user.
    pub fn es_factor(&self) -> u128 {
        self.es_factor
    }
}
