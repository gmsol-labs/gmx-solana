use anchor_lang::{prelude::*, ZeroCopy};
use gmsol_utils::{
    action::{ActionError, MAX_ACTION_FLAGS},
    InitSpace,
};

use crate::{
    events::Event,
    states::{NonceBytes, Seed},
    utils::pubkey::optional_address,
    CoreError,
};

pub use gmsol_utils::action::{ActionFlag, ActionState};

/// Action Header.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ActionHeader {
    version: u8,
    /// Action State.
    action_state: u8,
    /// The bump seed.
    pub(crate) bump: u8,
    flags: ActionFlagContainer,
    padding_0: [u8; 4],
    /// Action id.
    pub id: u64,
    /// Store.
    pub store: Pubkey,
    /// Market.
    pub market: Pubkey,
    /// Owner.
    pub owner: Pubkey,
    /// Nonce bytes.
    pub nonce: [u8; 32],
    /// Max execution lamports.
    pub(crate) max_execution_lamports: u64,
    /// Last updated timestamp.
    pub(crate) updated_at: i64,
    /// Last updated slot.
    pub(crate) updated_at_slot: u64,
    /// Creator.
    pub(crate) creator: Pubkey,
    /// Rent receiver.
    rent_receiver: Pubkey,
    /// The output funds receiver.
    receiver: Pubkey,
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    reserved: [u8; 256],
}

impl Default for ActionHeader {
    fn default() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

gmsol_utils::flags!(ActionFlag, MAX_ACTION_FLAGS, u8);

impl ActionHeader {
    /// Get action state.
    pub fn action_state(&self) -> Result<ActionState> {
        ActionState::try_from(self.action_state).map_err(|_| error!(CoreError::UnknownActionState))
    }

    fn set_action_state(&mut self, new_state: ActionState) {
        self.action_state = new_state.into();
    }

    /// Transition to Completed state.
    pub(crate) fn completed(&mut self) -> Result<()> {
        self.set_action_state(
            self.action_state()?
                .completed()
                .map_err(CoreError::from)
                .map_err(|err| error!(err))?,
        );
        Ok(())
    }

    /// Transition to Cancelled state.
    pub(crate) fn cancelled(&mut self) -> Result<()> {
        self.set_action_state(
            self.action_state()?
                .cancelled()
                .map_err(CoreError::from)
                .map_err(|err| error!(err))?,
        );
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

    /// Get the receiver.
    pub fn receiver(&self) -> Pubkey {
        *optional_address(&self.receiver).unwrap_or_else(|| self.owner())
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

    /// Get the bump.
    pub fn bump(&self) -> u8 {
        self.bump
    }

    /// Get the creator.
    /// We assume that the action account's address is derived from that address.
    pub fn creator(&self) -> &Pubkey {
        &self.creator
    }

    /// Get the rent receiver.
    pub fn rent_receiver(&self) -> &Pubkey {
        &self.rent_receiver
    }

    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn init(
        &mut self,
        id: u64,
        store: Pubkey,
        market: Pubkey,
        owner: Pubkey,
        receiver: Pubkey,
        nonce: [u8; 32],
        bump: u8,
        execution_lamports: u64,
        should_unwrap_native_token: bool,
    ) -> Result<()> {
        let clock = Clock::get()?;
        self.id = id;
        self.store = store;
        self.market = market;
        self.owner = owner;

        // Receiver must not be the `None` address.
        require!(
            optional_address(&receiver).is_some(),
            CoreError::InvalidArgument
        );

        self.receiver = receiver;
        self.nonce = nonce;
        self.max_execution_lamports = execution_lamports;
        self.updated_at = clock.unix_timestamp;
        self.updated_at_slot = clock.slot;
        self.bump = bump;
        // The creator defaults to the `owner`.
        self.creator = owner;
        // The rent receiver defaults to the `owner`.
        self.rent_receiver = owner;

        self.set_should_unwrap_native_token(should_unwrap_native_token);

        Ok(())
    }

    /// Set the creator.
    ///
    /// # CHECK
    /// - The address of this action account must be derived from this address.
    pub(crate) fn unchecked_set_creator(&mut self, creator: Pubkey) {
        self.creator = creator;
    }

    /// Set the rent receiver.
    pub(crate) fn set_rent_receiver(&mut self, rent_receiver: Pubkey) {
        self.rent_receiver = rent_receiver;
    }

    pub(crate) fn updated(&mut self) -> Result<()> {
        let clock = Clock::get()?;
        self.updated_at = clock.unix_timestamp;
        self.updated_at_slot = clock.slot;

        Ok(())
    }

    /// Returns whether the native token should be unwrapped.
    pub fn should_unwrap_native_token(&self) -> bool {
        self.flags.get_flag(ActionFlag::ShouldUnwrapNativeToken)
    }

    /// Set whether the native token should be unwrapped.
    ///
    /// Returns the previous vaule.
    fn set_should_unwrap_native_token(&mut self, should_unwrap: bool) -> bool {
        self.flags
            .set_flag(ActionFlag::ShouldUnwrapNativeToken, should_unwrap)
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

/// Closable Action.
pub trait Closable {
    /// Closed Event.
    type ClosedEvent: Event + InitSpace;

    /// To closed event.
    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent>;
}

impl From<ActionError> for CoreError {
    fn from(err: ActionError) -> Self {
        msg!("Action error: {}", err);
        match err {
            ActionError::PreconditionsAreNotMet(_) => Self::PreconditionsAreNotMet,
        }
    }
}
