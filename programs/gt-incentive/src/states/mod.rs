use anchor_lang::prelude::*;
use gmsol_store::CoreError;
use gmsol_utils::{
    airdrop::{
        AirdropConfigFlag, AirdropFlag, AirdropTargetFlag, MAX_AIRDROP_CONFIG_FLAGS,
        MAX_AIRDROP_FLAGS, MAX_AIRDROP_TARGET_FLAGS,
    },
    InitSpace,
};

/// Max number of operators in a single AirdropConfig.
pub const MAX_AIRDROP_OPERATORS: usize = 16;

gmsol_utils::flags!(AirdropConfigFlag, MAX_AIRDROP_CONFIG_FLAGS, u8);
gmsol_utils::flags!(AirdropFlag, MAX_AIRDROP_FLAGS, u8);
gmsol_utils::flags!(AirdropTargetFlag, MAX_AIRDROP_TARGET_FLAGS, u8);

/// PDA seed for the program's GT controller authority.
///
/// The PDA derived from this seed is the address that should be granted the
/// `GT_CONTROLLER` role in the store. The program signs the mint CPI using this PDA.
pub const GT_AUTHORITY_SEED: &[u8] = b"gt_authority";

/// A single operator entry inside [`AirdropConfig`].
///
/// Each operator is granted a timelock delay and a per-airdrop GT cap.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct OperatorEntry {
    /// The operator's wallet address.
    pub authority: Pubkey,
    /// Timelock delay in seconds granted to this operator (T in the spec).
    pub timelock_secs: u64,
    /// Max GT amount this operator may distribute in a single airdrop (N in the spec).
    pub max_airdrop_amount: u64,
    /// Whether this operator is currently enabled.
    pub is_enabled: u8,
    padding: [u8; 7],
}

impl InitSpace for OperatorEntry {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl OperatorEntry {
    fn new(authority: Pubkey, timelock_secs: u64, max_airdrop_amount: u64) -> Self {
        Self {
            authority,
            timelock_secs,
            max_airdrop_amount,
            is_enabled: 1,
            padding: [0; 7],
        }
    }

    /// Returns whether this operator is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled != 0
    }
}

/// Global singleton config for the GT airdrop system.
///
/// PDA seeds: `[SEED, store]`
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct AirdropConfig {
    pub(crate) bump: u8,
    flags: AirdropConfigFlagContainer,
    pub(crate) operator_count: u8,
    padding_0: [u8; 5],
    /// The store this config belongs to.
    pub(crate) store: Pubkey,
    /// The Gov address that is allowed to approve airdrops.
    /// Must hold GT_CONTROLLER role in the store's RoleStore.
    pub(crate) gov: Pubkey,
    pub(crate) operators: [OperatorEntry; MAX_AIRDROP_OPERATORS],
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 128],
}

impl InitSpace for AirdropConfig {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl AirdropConfig {
    pub const SEED: &'static [u8] = b"airdrop_config";

    /// Initialize this config.
    pub(crate) fn init(&mut self, bump: u8, store: &Pubkey, gov: &Pubkey) -> Result<()> {
        require!(
            !self.is_initialized(),
            CoreError::AirdropConfigAlreadyInitialized
        );
        self.bump = bump;
        self.store = *store;
        self.gov = *gov;
        self.flags.set_flag(AirdropConfigFlag::Initialized, true);
        Ok(())
    }

    /// Returns whether this config has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(AirdropConfigFlag::Initialized)
    }

    /// Returns the Gov address.
    pub fn gov(&self) -> &Pubkey {
        &self.gov
    }

    /// Find the operator entry for the given authority, if it exists.
    pub fn find_operator(&self, authority: &Pubkey) -> Option<&OperatorEntry> {
        self.operators[..self.operator_count as usize]
            .iter()
            .find(|op| &op.authority == authority)
    }

    /// Find the operator entry mutably.
    fn find_operator_mut(&mut self, authority: &Pubkey) -> Option<&mut OperatorEntry> {
        let count = self.operator_count as usize;
        self.operators[..count]
            .iter_mut()
            .find(|op| &op.authority == authority)
    }

    /// Add a new operator or update an existing one.
    pub(crate) fn upsert_operator(
        &mut self,
        authority: &Pubkey,
        timelock_secs: u64,
        max_airdrop_amount: u64,
        is_enabled: bool,
    ) -> Result<()> {
        if let Some(existing) = self.find_operator_mut(authority) {
            existing.timelock_secs = timelock_secs;
            existing.max_airdrop_amount = max_airdrop_amount;
            existing.is_enabled = u8::from(is_enabled);
        } else {
            let count = self.operator_count as usize;
            require!(
                count < MAX_AIRDROP_OPERATORS,
                CoreError::ExceedMaxLengthLimit
            );
            self.operators[count] =
                OperatorEntry::new(*authority, timelock_secs, max_airdrop_amount);
            self.operators[count].is_enabled = u8::from(is_enabled);
            self.operator_count = (count + 1) as u8;
        }
        Ok(())
    }

    /// Return the operator entry for `authority` if it is enabled.
    ///
    /// Returns [`CoreError::PermissionDenied`] if the operator does not exist or is disabled.
    pub(crate) fn get_enabled_operator(&self, authority: &Pubkey) -> Result<&OperatorEntry> {
        let op = self
            .find_operator(authority)
            .ok_or_else(|| error!(CoreError::PermissionDenied))?;
        require!(op.is_enabled(), CoreError::PermissionDenied);
        Ok(op)
    }
}

/// Represents a single airdrop campaign.
///
/// PDA seeds: `[SEED, store, operator, nonce]`
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct Airdrop {
    pub(crate) bump: u8,
    flags: AirdropFlagContainer,
    padding_0: [u8; 6],
    /// The store this airdrop belongs to.
    pub(crate) store: Pubkey,
    /// The operator who created this airdrop.
    pub(crate) operator: Pubkey,
    /// Unix timestamp after which this airdrop expires.
    pub(crate) expiry: i64,
    /// Unix timestamp after which users may claim (set at approval time: now + T).
    pub(crate) claimable_at: i64,
    /// Running total of GT across all uploaded targets.
    pub(crate) total_amount: u64,
    /// Number of targets uploaded so far.
    pub(crate) target_count: u64,
    /// Random nonce that makes this PDA unique per operator.
    pub(crate) nonce: [u8; 8],
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 128],
}

impl InitSpace for Airdrop {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Airdrop {
    pub const SEED: &'static [u8] = b"airdrop";

    /// Initialize a new airdrop.
    pub(crate) fn init(
        &mut self,
        bump: u8,
        store: &Pubkey,
        operator: &Pubkey,
        nonce: [u8; 8],
        expiry: i64,
    ) -> Result<()> {
        require!(!self.is_initialized(), CoreError::AirdropAlreadyInitialized);
        self.bump = bump;
        self.store = *store;
        self.operator = *operator;
        self.nonce = nonce;
        self.expiry = expiry;
        self.flags.set_flag(AirdropFlag::Initialized, true);
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(AirdropFlag::Initialized)
    }

    pub fn is_complete(&self) -> bool {
        self.flags.get_flag(AirdropFlag::Complete)
    }

    pub fn is_approved(&self) -> bool {
        self.flags.get_flag(AirdropFlag::Approved)
    }

    pub fn is_cancelled(&self) -> bool {
        self.flags.get_flag(AirdropFlag::Cancelled)
    }

    pub fn operator(&self) -> &Pubkey {
        &self.operator
    }

    pub fn expiry(&self) -> i64 {
        self.expiry
    }

    pub fn claimable_at(&self) -> i64 {
        self.claimable_at
    }

    pub fn total_amount(&self) -> u64 {
        self.total_amount
    }

    pub fn target_count(&self) -> u64 {
        self.target_count
    }

    /// Validate that this airdrop is still active (can receive targets or be marked complete).
    pub(crate) fn validate_active(&self) -> Result<()> {
        require!(self.is_initialized(), CoreError::AirdropNotInitialized);
        // `Cancelled` is a sticky terminal flag — report it ahead of any
        // intermediate state checks so callers get the truth ("this
        // campaign is dead") instead of a downstream detail.
        require!(!self.is_cancelled(), CoreError::AirdropCancelled);
        require!(!self.is_complete(), CoreError::AirdropAlreadyComplete);
        require!(!self.is_approved(), CoreError::AirdropAlreadyApproved);
        let clock = Clock::get()?;
        require_gt!(self.expiry, clock.unix_timestamp, CoreError::AirdropExpired);
        Ok(())
    }

    /// Validate that this airdrop is ready for approval.
    pub(crate) fn validate_approvable(&self) -> Result<()> {
        require!(self.is_initialized(), CoreError::AirdropNotInitialized);
        require!(!self.is_cancelled(), CoreError::AirdropCancelled);
        require!(self.is_complete(), CoreError::AirdropNotComplete);
        require!(!self.is_approved(), CoreError::AirdropAlreadyApproved);
        require_gt!(self.target_count, 0, CoreError::AirdropHasNoTargets);
        let clock = Clock::get()?;
        require_gt!(self.expiry, clock.unix_timestamp, CoreError::AirdropExpired);
        Ok(())
    }

    /// Validate that users may claim from this airdrop right now.
    pub(crate) fn validate_claimable(&self) -> Result<()> {
        require!(self.is_initialized(), CoreError::AirdropNotInitialized);
        require!(!self.is_cancelled(), CoreError::AirdropCancelled);
        require!(self.is_complete(), CoreError::AirdropNotComplete);
        require!(self.is_approved(), CoreError::AirdropNotApproved);
        let clock = Clock::get()?;
        require_gte!(
            clock.unix_timestamp,
            self.claimable_at,
            CoreError::AirdropTimelockNotElapsed
        );
        require_gt!(self.expiry, clock.unix_timestamp, CoreError::AirdropExpired);
        Ok(())
    }

    /// Add a target's amount to the running total.
    pub(crate) fn add_target(&mut self, amount: u64) -> Result<()> {
        self.total_amount = self
            .total_amount
            .checked_add(amount)
            .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
        self.target_count = self
            .target_count
            .checked_add(1)
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;
        Ok(())
    }

    /// Mark the target list as complete.
    pub(crate) fn mark_complete(&mut self) -> Result<()> {
        self.validate_active()?;
        require_gt!(self.target_count, 0, CoreError::AirdropHasNoTargets);
        self.flags.set_flag(AirdropFlag::Complete, true);
        Ok(())
    }

    /// Approve this airdrop. Sets `claimable_at = now + timelock_secs`.
    pub(crate) fn approve(&mut self, timelock_secs: u64) -> Result<()> {
        self.validate_approvable()?;
        let clock = Clock::get()?;
        let claimable_at = clock
            .unix_timestamp
            .checked_add(
                i64::try_from(timelock_secs).map_err(|_| error!(CoreError::ValueOverflow))?,
            )
            .ok_or_else(|| error!(CoreError::ValueOverflow))?;
        require_gte!(self.expiry, claimable_at, CoreError::AirdropExpiryTooClose);
        self.claimable_at = claimable_at;
        self.flags.set_flag(AirdropFlag::Approved, true);
        Ok(())
    }

    /// Cancel this airdrop. Only valid before approval; existing data is
    /// preserved so a subsequent `close` flow can refund rent to the operator.
    pub(crate) fn cancel(&mut self) -> Result<()> {
        require!(self.is_initialized(), CoreError::AirdropNotInitialized);
        require!(!self.is_cancelled(), CoreError::AirdropCancelled);
        require!(!self.is_approved(), CoreError::AirdropAlreadyApproved);
        self.flags.set_flag(AirdropFlag::Cancelled, true);
        Ok(())
    }
}

/// Represents a single recipient in an airdrop campaign.
///
/// PDA seeds: `[SEED, airdrop, recipient]`
///
/// The PDA derivation guarantees that each recipient appears at most once per airdrop.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct AirdropTarget {
    pub(crate) bump: u8,
    flags: AirdropTargetFlagContainer,
    padding_0: [u8; 6],
    /// The airdrop this target belongs to.
    pub(crate) airdrop: Pubkey,
    /// The recipient's wallet address.
    pub(crate) recipient: Pubkey,
    /// Amount of GT to mint when claimed.
    pub(crate) amount: u64,
    #[cfg_attr(feature = "debug", debug(skip))]
    reserved: [u8; 64],
}

impl InitSpace for AirdropTarget {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl AirdropTarget {
    pub const SEED: &'static [u8] = b"airdrop_target";

    /// Initialize this target.
    pub(crate) fn init(
        &mut self,
        bump: u8,
        airdrop: &Pubkey,
        recipient: &Pubkey,
        amount: u64,
    ) -> Result<()> {
        require!(
            !self.is_initialized(),
            CoreError::AirdropTargetAlreadyInitialized
        );
        require_gt!(amount, 0, CoreError::InvalidArgument);
        self.bump = bump;
        self.airdrop = *airdrop;
        self.recipient = *recipient;
        self.amount = amount;
        self.flags.set_flag(AirdropTargetFlag::Initialized, true);
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.flags.get_flag(AirdropTargetFlag::Initialized)
    }

    pub fn is_claimed(&self) -> bool {
        self.flags.get_flag(AirdropTargetFlag::Claimed)
    }

    pub fn recipient(&self) -> &Pubkey {
        &self.recipient
    }

    pub fn amount(&self) -> u64 {
        self.amount
    }

    /// Mark this target as claimed.
    pub(crate) fn claim(&mut self) -> Result<()> {
        require!(
            self.is_initialized(),
            CoreError::AirdropTargetNotInitialized
        );
        require!(!self.is_claimed(), CoreError::AirdropTargetAlreadyClaimed);
        self.flags.set_flag(AirdropTargetFlag::Claimed, true);
        Ok(())
    }
}
