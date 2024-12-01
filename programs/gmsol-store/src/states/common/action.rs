use anchor_lang::{prelude::*, ZeroCopy};
use gmsol_utils::InitSpace;

use crate::{
    constants,
    states::{NonceBytes, Seed},
    CoreError,
};

/// Action State.
#[non_exhaustive]
#[repr(u8)]
#[derive(
    Clone,
    Copy,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::Display,
    AnchorSerialize,
    AnchorDeserialize,
    InitSpace,
)]
#[strum(serialize_all = "snake_case")]
#[num_enum(error_type(name = CoreError, constructor = CoreError::unknown_action_state))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ActionState {
    /// Pending.
    Pending,
    /// Completed.
    Completed,
    /// Cancelled.
    Cancelled,
}

impl ActionState {
    /// Transition to Completed State.
    pub fn completed(self) -> Result<Self> {
        let Self::Pending = self else {
            return err!(CoreError::PreconditionsAreNotMet);
        };
        Ok(Self::Completed)
    }

    /// Transition to Cancelled State.
    pub fn cancelled(self) -> Result<Self> {
        let Self::Pending = self else {
            return err!(CoreError::PreconditionsAreNotMet);
        };
        Ok(Self::Cancelled)
    }

    /// Check if the state is completed or cancelled.
    pub fn is_completed_or_cancelled(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// Check if the state is pending.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
}

/// Action Header.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct ActionHeader {
    /// Action id.
    pub(crate) id: u64,
    /// Store.
    pub(crate) store: Pubkey,
    /// Market.
    pub(crate) market: Pubkey,
    /// Owner.
    pub(crate) owner: Pubkey,
    /// Nonce bytes.
    pub(crate) nonce: [u8; 32],
    /// Creator.
    pub(crate) creator: Pubkey,
    /// Max execution lamports.
    pub(crate) max_execution_lamports: u64,
    /// Last updated timestamp.
    pub(crate) updated_at: i64,
    /// Last updated slot.
    pub(crate) updated_at_slot: u64,
    /// Action State.
    action_state: u8,
    /// The bump seed.
    pub(crate) bump: u8,
    padding_0: [u8; 6],
}

impl ActionHeader {
    /// Get action state.
    pub fn action_state(&self) -> Result<ActionState> {
        ActionState::try_from(self.action_state).map_err(|err| error!(err))
    }

    fn set_action_state(&mut self, new_state: ActionState) {
        self.action_state = new_state.into();
    }

    /// Transition to Completed state.
    pub(crate) fn completed(&mut self) -> Result<()> {
        self.set_action_state(self.action_state()?.completed()?);
        Ok(())
    }

    /// Transition to Cancelled state.
    pub(crate) fn cancelled(&mut self) -> Result<()> {
        self.set_action_state(self.action_state()?.cancelled()?);
        Ok(())
    }

    /// Get action signer.
    pub(crate) fn signer(&self, seed: &'static [u8]) -> ActionSigner {
        ActionSigner::new(seed, self.store, *self.creator(), self.nonce, self.bump)
    }

    /// Get the owner.
    pub fn owner(&self) -> &Pubkey {
        &self.owner
    }

    /// Get the creator.
    /// We assume that the action account's address is derived from that address.
    pub fn creator(&self) -> &Pubkey {
        &self.creator
    }

    // Get the action id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get the store.
    pub fn store(&self) -> &Pubkey {
        &self.store
    }

    /// Get the market.
    pub fn market(&self) -> &Pubkey {
        &self.market
    }

    /// Get the nonce.
    pub fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }

    /// Get the bump.
    pub fn bump(&self) -> u8 {
        self.bump
    }

    /// Get max execution lamports.
    pub fn max_execution_lamports(&self) -> u64 {
        self.max_execution_lamports
    }

    /// Get last updated timestamp.
    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }

    /// Get last updated slot.
    pub fn updated_at_slot(&self) -> u64 {
        self.updated_at_slot
    }

    #[inline(never)]
    pub(crate) fn init(
        &mut self,
        id: u64,
        store: Pubkey,
        market: Pubkey,
        owner: Pubkey,
        nonce: [u8; 32],
        bump: u8,
        execution_lamports: u64,
    ) -> Result<()> {
        let clock = Clock::get()?;
        self.id = id;
        self.store = store;
        self.market = market;
        self.owner = owner;
        self.nonce = nonce;
        // The creator defaults to the `owner`.
        self.creator = owner;
        self.bump = bump;
        self.max_execution_lamports = execution_lamports;
        self.updated_at = clock.unix_timestamp;
        self.updated_at_slot = clock.slot;

        Ok(())
    }

    /// Set the creator.
    ///
    /// # CHECK
    /// - The address of this action account must be derived from this address.
    pub(crate) fn unchecked_set_creator(&mut self, creator: Pubkey) {
        self.creator = creator;
    }

    pub(crate) fn updated(&mut self) -> Result<()> {
        let clock = Clock::get()?;
        self.updated_at = clock.unix_timestamp;
        self.updated_at_slot = clock.slot;

        Ok(())
    }
}

/// Action Signer.
pub struct ActionSigner {
    seed: &'static [u8],
    store: Pubkey,
    owner: Pubkey,
    nonce: NonceBytes,
    bump_bytes: [u8; 1],
}

impl ActionSigner {
    /// Create a new action signer.
    pub fn new(
        seed: &'static [u8],
        store: Pubkey,
        owner: Pubkey,
        nonce: NonceBytes,
        bump: u8,
    ) -> Self {
        Self {
            seed,
            store,
            owner,
            nonce,
            bump_bytes: [bump],
        }
    }

    /// As signer seeds.
    pub fn as_seeds(&self) -> [&[u8]; 5] {
        [
            self.seed,
            self.store.as_ref(),
            self.owner.as_ref(),
            &self.nonce,
            &self.bump_bytes,
        ]
    }
}

/// Action.
pub trait Action {
    /// Min execution lamports.
    const MIN_EXECUTION_LAMPORTS: u64;

    /// Get the header.
    fn header(&self) -> &ActionHeader;
}

/// Extentsion trait for [`Action`].
pub trait ActionExt: Action {
    /// Action signer.
    fn signer(&self) -> ActionSigner
    where
        Self: Seed,
    {
        self.header().signer(Self::SEED)
    }

    /// Execution lamports.
    fn execution_lamports(&self, execution_lamports: u64) -> u64 {
        execution_lamports.min(self.header().max_execution_lamports)
    }

    /// Validate balance.
    fn validate_balance(account: &AccountLoader<Self>, execution_lamports: u64) -> Result<()>
    where
        Self: ZeroCopy + Owner + InitSpace,
    {
        require_gte!(
            execution_lamports,
            Self::MIN_EXECUTION_LAMPORTS,
            CoreError::NotEnoughExecutionFee
        );
        let balance = account.get_lamports().saturating_sub(execution_lamports);
        let rent = Rent::get()?;
        require!(
            rent.is_exempt(balance, 8 + Self::INIT_SPACE),
            CoreError::NotEnoughExecutionFee
        );
        Ok(())
    }
}

impl<T: Action> ActionExt for T {}

/// Action Parameters.
pub trait ActionParams {
    /// Get max allowed execution fee in lamports.
    fn execution_lamports(&self) -> u64;
}

/// Action Event.
pub trait ActionEvent: InitSpace + anchor_lang::Event {
    /// Emit this event through CPI. This is a manual implementation of `emit_cpi!`.
    fn emit_cpi(&self, event_authority: AccountInfo, event_authority_bump: u8) -> Result<()> {
        use anchor_lang::solana_program::instruction::Instruction;

        let disc = anchor_lang::event::EVENT_IX_TAG_LE;
        let mut ix_data = Vec::with_capacity(16 + Self::INIT_SPACE);
        ix_data.extend_from_slice(&disc);
        ix_data.extend_from_slice(&Self::DISCRIMINATOR);
        self.serialize(&mut ix_data)?;
        let ix = Instruction {
            program_id: crate::ID,
            accounts: vec![AccountMeta::new_readonly(*event_authority.key, true)],
            data: ix_data,
        };
        anchor_lang::solana_program::program::invoke_signed(
            &ix,
            &[event_authority],
            &[&[constants::EVENT_AUTHORITY_SEED, &[event_authority_bump]]],
        )?;
        Ok(())
    }
}

/// Closable Action.
pub trait Closable {
    /// Closed Event.
    type ClosedEvent: ActionEvent;

    /// To closed event.
    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent>;
}
