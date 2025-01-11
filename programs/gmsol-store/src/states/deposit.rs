use anchor_lang::prelude::*;
use gmsol_utils::InitSpace;

use crate::{events::DepositRemoved, states::MarketConfigKey, CoreError};

use super::{
    common::{
        action::{Action, ActionHeader, Closable},
        swap::SwapParams,
        token::TokenAndAccount,
    },
    Market, Seed,
};

/// Deposit.
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct Deposit {
    /// Header.
    pub(crate) header: ActionHeader,
    /// Token accounts.
    pub(crate) tokens: TokenAccounts,
    /// Deposit params.
    pub(crate) params: DepositParams,
    /// Swap params.
    pub(crate) swap: SwapParams,
    padding_1: [u8; 4],
    reserve: [u8; 128],
}

/// PDA for first deposit owner.
pub fn find_first_deposit_receiver_pda(store_program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[Deposit::FIRST_DEPOSIT_RECEIVER_SEED], store_program_id)
}

impl InitSpace for Deposit {
    const INIT_SPACE: usize = std::mem::size_of::<Self>();
}

impl Closable for Deposit {
    type ClosedEvent = DepositRemoved;

    fn to_closed_event(&self, address: &Pubkey, reason: &str) -> Result<Self::ClosedEvent> {
        DepositRemoved::new(
            self.header.id,
            self.header.store,
            *address,
            self.tokens.market_token(),
            self.header.owner,
            self.header.action_state()?,
            reason,
        )
    }
}

impl Deposit {
    /// Fisrt Deposit Receiver Seed.
    pub const FIRST_DEPOSIT_RECEIVER_SEED: &'static [u8] = b"first_deposit_receiver";

    /// Get first deposit receiver.
    pub fn first_deposit_receiver() -> Pubkey {
        find_first_deposit_receiver_pda(&crate::ID).0
    }

    /// Get tokens.
    pub fn tokens(&self) -> &TokenAccounts {
        &self.tokens
    }

    /// Get swap params.
    pub fn swap(&self) -> &SwapParams {
        &self.swap
    }

    /// Validate the deposit params for execution.
    pub(crate) fn validate_for_execution(
        &self,
        market_token: &AccountInfo,
        market: &Market,
    ) -> Result<()> {
        use anchor_spl::token::accessor::amount;

        require_eq!(
            *market_token.key,
            self.tokens().market_token(),
            CoreError::MarketTokenMintMismatched
        );

        let supply = amount(market_token)?;

        if supply == 0 {
            Self::validate_first_deposit(
                &self.header.receiver(),
                self.params.min_market_token_amount,
                market,
            )?;
        }

        Ok(())
    }

    pub(crate) fn validate_first_deposit(
        receiver: &Pubkey,
        min_amount: u64,
        market: &Market,
    ) -> Result<()> {
        let min_tokens_for_first_deposit =
            market.get_config_by_key(MarketConfigKey::MinTokensForFirstDeposit);

        // Skip first deposit check if the amount is zero.
        if *min_tokens_for_first_deposit == 0 {
            return Ok(());
        }

        require_eq!(
            *receiver,
            Self::first_deposit_receiver(),
            CoreError::InvalidReceiverForFirstDeposit
        );

        require_gte!(
            min_amount as u128,
            *min_tokens_for_first_deposit,
            CoreError::NotEnoughMarketTokenAmountForFirstDeposit
        );

        Ok(())
    }
}

impl Seed for Deposit {
    /// Seed.
    const SEED: &'static [u8] = b"deposit";
}

impl Action for Deposit {
    const MIN_EXECUTION_LAMPORTS: u64 = 200_000;

    fn header(&self) -> &ActionHeader {
        &self.header
    }
}

/// Token Accounts.
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct TokenAccounts {
    /// Initial long token accounts.
    pub initial_long_token: TokenAndAccount,
    /// Initial short token accounts.
    pub initial_short_token: TokenAndAccount,
    /// Market token account.
    pub(crate) market_token: TokenAndAccount,
}

impl TokenAccounts {
    /// Get market token.
    pub fn market_token(&self) -> Pubkey {
        self.market_token.token().expect("must exist")
    }

    /// Get market token account.
    pub fn market_token_account(&self) -> Pubkey {
        self.market_token.account().expect("must exist")
    }
}

/// Deposit Params.
#[cfg_attr(feature = "debug", derive(Debug))]
#[account(zero_copy)]
pub struct DepositParams {
    /// The amount of initial long tokens to deposit.
    pub(crate) initial_long_token_amount: u64,
    /// The amount of initial short tokens to deposit.
    pub(crate) initial_short_token_amount: u64,
    /// The minimum acceptable amount of market tokens to receive.
    pub(crate) min_market_token_amount: u64,
    reserved: [u8; 64],
}

impl Default for DepositParams {
    fn default() -> Self {
        Self {
            initial_long_token_amount: 0,
            initial_short_token_amount: 0,
            min_market_token_amount: 0,
            reserved: [0; 64],
        }
    }
}

impl DepositParams {
    pub(crate) fn validate_market_token_amount(&self, minted: u64) -> Result<()> {
        require_gte!(
            minted,
            self.min_market_token_amount,
            CoreError::InsufficientOutputAmount
        );
        Ok(())
    }
}
