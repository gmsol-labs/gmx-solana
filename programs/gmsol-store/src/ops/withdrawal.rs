use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, TokenAccount};
use typed_builder::TypedBuilder;

use crate::{
    states::{
        common::action::{Action, ActionParams},
        market::revertible::Revertible,
        withdrawal::Withdrawal,
        Market, NonceBytes, Oracle, Store, ValidateOracleTime,
    },
    CoreError, CoreResult,
};

use super::market::RevertibleLiquidityMarketOperation;

/// Create Withdrawal Params.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateWithdrawalParams {
    /// Execution fee in lamports.
    pub execution_lamports: u64,
    /// The length of the swap path for long token.
    pub long_token_swap_path_length: u8,
    /// The length of the swap path for short token.
    pub short_token_swap_path_length: u8,
    /// Market token amount to burn.
    pub market_token_amount: u64,
    /// The minimum acceptable final long token amount to receive.
    pub min_long_token_amount: u64,
    /// The minimum acceptable final short token amount to receive.
    pub min_short_token_amount: u64,
}

impl ActionParams for CreateWithdrawalParams {
    fn execution_lamports(&self) -> u64 {
        self.execution_lamports
    }
}

/// Operation for creating a withdrawal.
#[derive(TypedBuilder)]
pub(crate) struct CreateWithdrawalOperation<'a, 'info> {
    withdrawal: AccountLoader<'info, Withdrawal>,
    market: AccountLoader<'info, Market>,
    store: AccountLoader<'info, Store>,
    owner: &'a AccountInfo<'info>,
    nonce: &'a NonceBytes,
    bump: u8,
    final_long_token: &'a Account<'info, TokenAccount>,
    final_short_token: &'a Account<'info, TokenAccount>,
    market_token: &'a Account<'info, TokenAccount>,
    params: &'a CreateWithdrawalParams,
    swap_paths: &'info [AccountInfo<'info>],
}

impl<'a, 'info> CreateWithdrawalOperation<'a, 'info> {
    /// Execute.
    pub(crate) fn execute(self) -> Result<()> {
        self.market.load()?.validate(&self.store.key())?;
        self.validate_params_excluding_swap()?;

        let Self {
            withdrawal,
            market,
            store,
            owner,
            nonce,
            bump,
            final_long_token,
            final_short_token,
            market_token,
            params,
            swap_paths,
        } = self;

        let id = market.load_mut()?.indexer_mut().next_withdrawal_id()?;

        let mut withdrawal = withdrawal.load_init()?;

        // Initialize header.
        withdrawal.header.init(
            id,
            store.key(),
            market.key(),
            owner.key(),
            *nonce,
            bump,
            params.execution_lamports,
        )?;

        // Initialize tokens.
        withdrawal.tokens.market_token.init(market_token);
        withdrawal.tokens.final_long_token.init(final_long_token);
        withdrawal.tokens.final_short_token.init(final_short_token);

        // Initialize params.
        withdrawal.params.market_token_amount = params.market_token_amount;
        withdrawal.params.min_long_token_amount = params.min_long_token_amount;
        withdrawal.params.min_short_token_amount = params.min_short_token_amount;

        // Initialize swap paths.
        let market = market.load()?;
        let meta = market.meta();
        withdrawal.swap.validate_and_init(
            &*market,
            params.long_token_swap_path_length,
            params.short_token_swap_path_length,
            swap_paths,
            &store.key(),
            (&meta.long_token_mint, &meta.short_token_mint),
            (&final_long_token.mint, &final_short_token.mint),
        )?;

        Ok(())
    }

    fn validate_params_excluding_swap(&self) -> Result<()> {
        let params = &self.params;
        require!(params.market_token_amount != 0, CoreError::EmptyWithdrawal);
        require_gte!(
            self.market_token.amount,
            params.market_token_amount,
            CoreError::NotEnoughTokenAmount,
        );

        require_gte!(
            params.execution_lamports,
            Withdrawal::MIN_EXECUTION_LAMPORTS,
            CoreError::NotEnoughExecutionFee
        );

        require_gte!(
            self.withdrawal.get_lamports(),
            params.execution_lamports,
            CoreError::NotEnoughExecutionFee
        );

        Ok(())
    }
}

/// Operation for executing a withdrawal.
#[derive(TypedBuilder)]
pub(crate) struct ExecuteWithdrawalOperation<'a, 'info> {
    store: &'a AccountLoader<'info, Store>,
    market: &'a AccountLoader<'info, Market>,
    market_token_mint: &'a mut Account<'info, Mint>,
    market_token_vault: AccountInfo<'info>,
    withdrawal: &'a AccountLoader<'info, Withdrawal>,
    oracle: &'a Oracle,
    remaining_accounts: &'info [AccountInfo<'info>],
    throw_on_execution_error: bool,
    token_program: AccountInfo<'info>,
}

impl<'a, 'info> ExecuteWithdrawalOperation<'a, 'info> {
    pub(crate) fn execute(self) -> Result<Option<(u64, u64)>> {
        let throw_on_execution_error = self.throw_on_execution_error;
        match self.validate_oracle() {
            Ok(()) => {}
            Err(CoreError::OracleTimestampsAreLargerThanRequired) if !throw_on_execution_error => {
                msg!(
                    "Withdrawal expired at {}",
                    self.oracle_updated_before()
                        .ok()
                        .flatten()
                        .expect("must have an expiration time"),
                );
                return Ok(None);
            }
            Err(err) => {
                return Err(error!(err));
            }
        }
        match self.perform_withdrawal() {
            Ok(res) => Ok(Some(res)),
            Err(err) if !throw_on_execution_error => {
                msg!("Execute withdrawal error: {}", err);
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    fn validate_oracle(&self) -> CoreResult<()> {
        self.oracle.validate_time(self)
    }

    #[inline(never)]
    fn perform_withdrawal(self) -> Result<(u64, u64)> {
        self.market.load()?.validate(&self.store.key())?;

        let withdrawal = self.withdrawal.load()?;

        let mut market = RevertibleLiquidityMarketOperation::new(
            self.store,
            self.oracle,
            self.market,
            self.market_token_mint,
            self.token_program,
            Some(&withdrawal.swap),
            self.remaining_accounts,
        )?;

        let executed = market.op()?.unchecked_withdraw(
            &self.market_token_vault,
            &withdrawal.params,
            (
                withdrawal.tokens.final_long_token(),
                withdrawal.tokens.final_short_token(),
            ),
        )?;

        let final_output_amounts = executed.output;

        executed.commit();

        Ok(final_output_amounts)
    }
}

impl<'a, 'info> ValidateOracleTime for ExecuteWithdrawalOperation<'a, 'info> {
    fn oracle_updated_after(&self) -> CoreResult<Option<i64>> {
        Ok(Some(
            self.withdrawal
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header()
                .updated_at,
        ))
    }

    fn oracle_updated_before(&self) -> CoreResult<Option<i64>> {
        let ts = self
            .store
            .load()
            .map_err(|_| CoreError::LoadAccountError)?
            .request_expiration_at(
                self.withdrawal
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?
                    .header()
                    .updated_at,
            )?;
        Ok(Some(ts))
    }

    fn oracle_updated_after_slot(&self) -> CoreResult<Option<u64>> {
        Ok(Some(
            self.withdrawal
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header()
                .updated_at_slot,
        ))
    }
}
