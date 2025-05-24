use anchor_lang::prelude::*;

use gmsol_model::{BaseMarketExt, ClockKind, PnlFactorKind};

use crate::{constants, states::Oracle, CoreError, CoreResult, ModelError};

use super::{HasMarketMeta, Market};

/// Extension trait for validating market balances.
pub trait ValidateMarketBalances:
    gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }, Num = u128>
    + gmsol_model::Bank<Pubkey, Num = u64>
    + HasMarketMeta
{
    /// Validate market balance for the given token.
    ///
    /// # Notes
    /// Unlike the Solidity version, this function does not actually verify
    /// the token balance but only checks the balance recorded in the market state.
    /// This is because we use a shared vaults design, making it unlikely to sum all
    /// markets in a single instruction and then verify the vault's balance.
    ///
    /// The reason we adopt a shared vaults design is to avoid performing a large number
    /// of CPIs when executing swaps along the swap path, which could easily lead to heap
    /// memory overflows if we used Solana's default heap allocator.
    fn validate_market_balance_for_the_given_token(
        &self,
        token: &Pubkey,
        excluded: u64,
    ) -> gmsol_model::Result<()> {
        let is_long_token = self.market_meta().to_token_side(token)?;
        let is_pure = self.is_pure();

        let balance = u128::from(self.balance_excluding(token, &excluded)?);

        let mut min_token_balance = self
            .expected_min_token_balance_excluding_collateral_amount_for_one_token_side(
                is_long_token,
            )?;

        // Since the `min_token_balance` only accounts for one side, we need to include the other side
        // for a pure market.
        if is_pure {
            min_token_balance = min_token_balance
                    .checked_add(
                        self.expected_min_token_balance_excluding_collateral_amount_for_one_token_side(
                            !is_long_token,
                        )?,
                    )
                    .ok_or(gmsol_model::Error::Computation(
                        "validate balance: overflow while adding the min token balance for the other side",
                    ))?;
        }

        crate::debug_msg!(
            "[Validation] validating min token balance: {} >= {}",
            balance,
            min_token_balance
        );
        if balance < min_token_balance {
            return Err(gmsol_model::Error::InvalidTokenBalance(
                "Less than expected min token balance excluding collateral amount",
                min_token_balance.to_string(),
                balance.to_string(),
            ));
        }

        let mut collateral_amount =
            self.total_collateral_amount_for_one_token_side(is_long_token)?;

        // Since the `collateral_amount` only accounts for one side, we need to include the other side
        // for a pure market.
        if is_pure {
            collateral_amount = collateral_amount
                    .checked_add(self.total_collateral_amount_for_one_token_side(!is_long_token)?)
                    .ok_or(gmsol_model::Error::Computation(
                        "validate balance: overflow while adding the collateral amount for the other side",
                    ))?;
        }

        crate::debug_msg!(
            "[Validation] validating collateral amount: {} >= {}",
            balance,
            collateral_amount
        );
        if balance < collateral_amount {
            return Err(gmsol_model::Error::InvalidTokenBalance(
                "Less than total collateral amount",
                collateral_amount.to_string(),
                balance.to_string(),
            ));
        }

        // We don't have to validate the claimable funding amount since they are claimed immediately.

        Ok(())
    }

    /// Validate market balances.
    fn validate_market_balances(
        &self,
        mut long_excluding_amount: u64,
        mut short_excluding_amount: u64,
    ) -> Result<()> {
        if self.is_pure() {
            let total = long_excluding_amount
                .checked_add(short_excluding_amount)
                .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            long_excluding_amount = total;
            short_excluding_amount = 0;
        }
        let meta = self.market_meta();
        self.validate_market_balance_for_the_given_token(
            &meta.long_token_mint,
            long_excluding_amount,
        )
        .map_err(ModelError::from)?;
        // Skip the validation for short token if this is a pure market,
        // where the long token and short token are the same.
        if !self.is_pure() {
            self.validate_market_balance_for_the_given_token(
                &meta.short_token_mint,
                short_excluding_amount,
            )
            .map_err(ModelError::from)?;
        }
        Ok(())
    }

    /// Validate market balances excluding the given token amounts.
    fn validate_market_balances_excluding_the_given_token_amounts(
        &self,
        first_token: &Pubkey,
        second_token: &Pubkey,
        first_excluding_amount: u64,
        second_excluding_amount: u64,
    ) -> Result<()> {
        let mut long_excluding_amount = 0u64;
        let mut short_excluding_amount = 0u64;

        for (token, amount) in [
            (first_token, first_excluding_amount),
            (second_token, second_excluding_amount),
        ] {
            if amount == 0 {
                continue;
            }
            let is_long = self
                .market_meta()
                .to_token_side(token)
                .map_err(CoreError::from)?;
            if is_long {
                long_excluding_amount = long_excluding_amount
                    .checked_add(amount)
                    .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            } else {
                short_excluding_amount = short_excluding_amount
                    .checked_add(amount)
                    .ok_or_else(|| error!(CoreError::TokenAmountOverflow))?;
            }
        }

        self.validate_market_balances(long_excluding_amount, short_excluding_amount)
    }
}

impl<
        M: gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }, Num = u128>
            + gmsol_model::Bank<Pubkey, Num = u64>
            + HasMarketMeta,
    > ValidateMarketBalances for M
{
}

/// Trait for defining operations related to auto-deleveraging.
pub trait Adl {
    /// Validate if the ADL can be executed.
    fn validate_adl(&self, oracle: &Oracle, is_long: bool, max_staleness: u64) -> CoreResult<()>;

    /// Latest ADL time.
    fn latest_adl_time(&self, is_long: bool) -> CoreResult<i64>;

    fn update_adl_state(&mut self, oracle: &Oracle, is_long: bool) -> Result<()>;
}

impl Adl for Market {
    fn latest_adl_time(&self, is_long: bool) -> CoreResult<i64> {
        let clock = if is_long {
            ClockKind::AdlForLong
        } else {
            ClockKind::AdlForShort
        };
        self.clock(clock).ok_or(CoreError::NotFound)
    }

    fn validate_adl(&self, oracle: &Oracle, is_long: bool, max_staleness: u64) -> CoreResult<()> {
        if !self.is_adl_enabled(is_long) {
            return Err(CoreError::AdlNotEnabled);
        }
        if oracle
            .max_oracle_ts()
            .saturating_add_unsigned(max_staleness)
            < self.latest_adl_time(is_long)?
        {
            return Err(CoreError::OracleTimestampsAreSmallerThanRequired);
        }
        Ok(())
    }

    fn update_adl_state(&mut self, oracle: &Oracle, is_long: bool) -> Result<()> {
        if oracle.max_oracle_ts() < self.latest_adl_time(is_long)? {
            return err!(CoreError::OracleTimestampsAreSmallerThanRequired);
        }
        require!(self.is_enabled(), CoreError::DisabledMarket);
        let prices = self.prices(oracle)?;
        let is_exceeded = self
            .pnl_factor_exceeded(&prices, PnlFactorKind::ForAdl, is_long)
            .map_err(ModelError::from)?
            .is_some();
        self.set_adl_enabled(is_long, is_exceeded);
        let kind = if is_long {
            ClockKind::AdlForLong
        } else {
            ClockKind::AdlForShort
        };
        let clock = self
            .state
            .clocks
            .get_mut(kind)
            .ok_or_else(|| error!(CoreError::NotFound))?;
        *clock = Clock::get()?.unix_timestamp;
        Ok(())
    }
}
