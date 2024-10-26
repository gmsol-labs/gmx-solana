use std::{borrow::BorrowMut, cell::RefMut};

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, TokenAccount};
use gmsol_model::Bank;
use gmsol_utils::InitSpace;
use typed_builder::TypedBuilder;

use crate::{
    states::{
        common::action::{Action, ActionExt, ActionParams},
        deposit::DepositParams,
        market::revertible::Revertible,
        withdrawal::WithdrawalParams,
        Market, NonceBytes, Oracle, Shift, Store, ValidateOracleTime,
    },
    CoreError, CoreResult, ModelError,
};

use super::market::RevertibleLiquidityMarketOperation;

/// Create Shift Params.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateShiftParams {
    /// Execution fee in lamports.
    pub execution_lamports: u64,
    /// From market token amount.
    pub from_market_token_amount: u64,
    /// The minimum acceptable to market token amount to receive.
    pub min_to_market_token_amount: u64,
}

impl ActionParams for CreateShiftParams {
    fn execution_lamports(&self) -> u64 {
        self.execution_lamports
    }
}

/// Operation for creating a shift.
#[derive(TypedBuilder)]
pub struct CreateShiftOperation<'a, 'info, T>
where
    T: anchor_lang::ZeroCopy + anchor_lang::Owner,
{
    store: &'a AccountLoader<'info, Store>,
    owner: AccountInfo<'info>,
    shift: &'a AccountLoader<'info, T>,
    from_market: &'a AccountLoader<'info, Market>,
    from_market_token_account: &'a Account<'info, TokenAccount>,
    to_market: &'a AccountLoader<'info, Market>,
    to_market_token_account: &'a Account<'info, TokenAccount>,
    nonce: &'a NonceBytes,
    bump: u8,
    params: &'a CreateShiftParams,
}

impl<'a, 'info, T> CreateShiftOperation<'a, 'info, T>
where
    T: anchor_lang::ZeroCopy + anchor_lang::Owner + Action + InitSpace,
    T: BorrowMut<Shift>,
{
    pub(crate) fn execute(self) -> Result<()> {
        self.validate_markets()?;
        self.validate_params()?;

        let id = self.from_market.load_mut()?.indexer_mut().next_shift_id()?;

        let mut shift = RefMut::map(self.shift.load_init()?, |shift| shift.borrow_mut());

        // Initialize the header.
        shift.header.init(
            id,
            self.store.key(),
            self.from_market.key(),
            self.owner.key(),
            *self.nonce,
            self.bump,
            self.params.execution_lamports,
        )?;

        // Initialize tokens.
        shift
            .tokens
            .from_market_token
            .init(self.from_market_token_account);
        shift
            .tokens
            .to_market_token
            .init(self.to_market_token_account);
        {
            let market = self.from_market.load()?;
            shift.tokens.long_token = market.meta().long_token_mint;
            shift.tokens.short_token = market.meta().short_token_mint;
        }

        // Initialize params.
        shift.params.from_market_token_amount = self.params.from_market_token_amount;
        shift.params.min_to_market_token_amount = self.params.min_to_market_token_amount;

        Ok(())
    }

    fn validate_markets(&self) -> Result<()> {
        require!(
            self.from_market.key() != self.to_market.key(),
            CoreError::InvalidShiftMarkets,
        );

        let from_market = self.from_market.load()?;
        let to_market = self.to_market.load()?;

        let store = &self.store.key();
        from_market.validate(store)?;
        to_market.validate(store)?;

        from_market.validate_shiftable(&to_market)?;

        require_eq!(
            from_market.meta().market_token_mint,
            self.from_market_token_account.mint,
            CoreError::MarketTokenMintMismatched,
        );

        require_eq!(
            to_market.meta().market_token_mint,
            self.to_market_token_account.mint,
            CoreError::MarketTokenMintMismatched,
        );
        Ok(())
    }

    fn validate_params(&self) -> Result<()> {
        let params = &self.params;

        require!(params.from_market_token_amount != 0, CoreError::EmptyShift);
        require_gte!(
            self.from_market_token_account.amount,
            params.from_market_token_amount,
            CoreError::NotEnoughTokenAmount
        );

        ActionExt::validate_balance(self.shift, params.execution_lamports)?;
        Ok(())
    }
}

/// Operation for executing a shift.
#[derive(TypedBuilder)]
pub struct ExecuteShiftOperation<'a, 'info> {
    store: &'a AccountLoader<'info, Store>,
    oracle: &'a Oracle,
    shift: &'a AccountLoader<'info, Shift>,
    from_market: &'a AccountLoader<'info, Market>,
    from_market_token_mint: &'a mut Account<'info, Mint>,
    from_market_token_vault: AccountInfo<'info>,
    to_market: &'a AccountLoader<'info, Market>,
    to_market_token_mint: &'a mut Account<'info, Mint>,
    to_market_token_account: AccountInfo<'info>,
    throw_on_execution_error: bool,
    token_program: AccountInfo<'info>,
}

impl<'a, 'info> ExecuteShiftOperation<'a, 'info> {
    pub(crate) fn execute(self) -> Result<bool> {
        let throw_on_execution_error = self.throw_on_execution_error;

        match self.validate_oracle() {
            Ok(()) => {}
            Err(CoreError::OracleTimestampsAreLargerThanRequired) if !throw_on_execution_error => {
                msg!(
                    "shift expired at {}",
                    self.oracle_updated_before()
                        .ok()
                        .flatten()
                        .expect("must have an expiration time"),
                );
                return Ok(false);
            }
            Err(err) => {
                return Err(error!(err));
            }
        }
        match self.perfrom_shift() {
            Ok(()) => Ok(true),
            Err(err) if !throw_on_execution_error => {
                msg!("Execute shift error: {}", err);
                Ok(false)
            }
            Err(err) => Err(err),
        }
    }

    fn validate_oracle(&self) -> CoreResult<()> {
        self.oracle.validate_time(self)
    }

    fn validate_markets_and_shift(&self) -> Result<(Pubkey, Pubkey)> {
        require!(
            self.from_market.key() != self.to_market.key(),
            CoreError::Internal
        );

        let from_market = self.from_market.load()?;
        let to_market = self.to_market.load()?;

        let meta = from_market.validated_meta(&self.store.key())?;
        to_market.validate(&self.store.key())?;

        from_market.validate_shiftable(&to_market)?;

        self.shift
            .load()?
            .validate_for_execution(&self.to_market_token_mint.to_account_info(), &to_market)?;

        Ok((meta.long_token_mint, meta.short_token_mint))
    }

    #[inline(never)]
    fn perfrom_shift(self) -> Result<()> {
        let (long_token, short_token) = self.validate_markets_and_shift()?;

        let shift = self.shift.load()?;

        let mut from_market = RevertibleLiquidityMarketOperation::new(
            self.store,
            self.oracle,
            self.from_market,
            self.from_market_token_mint,
            self.token_program.clone(),
            None,
            &[],
        )?;

        let mut to_market = RevertibleLiquidityMarketOperation::new(
            self.store,
            self.oracle,
            self.to_market,
            self.to_market_token_mint,
            self.token_program,
            None,
            &[],
        )?;

        // Perform the shift-withdrawal.
        let mut from_market = {
            let mut params = WithdrawalParams::default();
            params.market_token_amount = shift.params.from_market_token_amount;
            from_market.op()?.unchekced_withdraw(
                &self.from_market_token_vault,
                &params,
                (long_token, short_token),
            )?
        };
        let (long_amount, short_amount) = from_market.output;

        let mut to_market = to_market.op()?;

        // Transfer tokens from the `from_market` to `to_market`.
        // The vaults are assumed to be shared.
        {
            from_market
                .market_mut()
                .record_transferred_out_by_token(&long_token, &long_amount)
                .map_err(ModelError::from)?;
            to_market
                .market_mut()
                .record_transferred_in_by_token(&long_token, &long_amount)
                .map_err(ModelError::from)?;

            from_market
                .market_mut()
                .record_transferred_out_by_token(&short_token, &short_amount)
                .map_err(ModelError::from)?;
            to_market
                .market_mut()
                .record_transferred_in_by_token(&short_token, &short_amount)
                .map_err(ModelError::from)?;
        }

        // Perform the shift-deposit.
        let to_market = {
            let mut params = DepositParams::default();
            params.initial_long_token_amount = long_amount;
            params.initial_short_token_amount = short_amount;
            params.min_market_token_amount = shift.params.min_to_market_token_amount;
            to_market.unchecked_deposit(&self.to_market_token_account, &params, (None, None))?
        };

        // Commit the changes.
        from_market.commit();
        to_market.commit();

        Ok(())
    }
}

impl<'a, 'info> ValidateOracleTime for ExecuteShiftOperation<'a, 'info> {
    fn oracle_updated_after(&self) -> CoreResult<Option<i64>> {
        Ok(Some(
            self.shift
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
                self.shift
                    .load()
                    .map_err(|_| CoreError::LoadAccountError)?
                    .header()
                    .updated_at,
            )?;
        Ok(Some(ts))
    }

    fn oracle_updated_after_slot(&self) -> CoreResult<Option<u64>> {
        Ok(Some(
            self.shift
                .load()
                .map_err(|_| CoreError::LoadAccountError)?
                .header()
                .updated_at_slot,
        ))
    }
}
