use anchor_lang::prelude::*;
use anchor_spl::token::Mint;
use gmsol_model::params::{position::PositionImpactDistributionParams, PriceImpactParams};

use crate::{
    constants,
    states::{HasMarketMeta, Store},
    utils::internal::TransferUtils,
};

use super::{
    market::{RevertibleMarket, SwapPricingKind},
    Revertible, Revision,
};

/// Convert a [`RevertibleMarket`] to a [`LiquidityMarketMut`](gmsol_model::LiquidityMarketMut).
pub struct RevertibleLiquidityMarket<'a, 'info> {
    base: RevertibleMarket<'a, 'info>,
    token_program: &'a AccountInfo<'info>,
    store: &'a AccountLoader<'info, Store>,
    market_token: &'a Account<'info, Mint>,
    receiver: Option<&'a AccountInfo<'info>>,
    vault: Option<&'a AccountInfo<'info>>,
    to_mint: u64,
    to_burn: u64,
}

impl<'a, 'info> RevertibleLiquidityMarket<'a, 'info> {
    pub(crate) fn from_revertible_market(
        market: RevertibleMarket<'a, 'info>,
        market_token: &'a Account<'info, Mint>,
        token_program: &'a AccountInfo<'info>,
        store: &'a AccountLoader<'info, Store>,
    ) -> Result<Self> {
        Ok(Self {
            base: market,
            token_program,
            store,
            market_token,
            receiver: None,
            vault: None,
            to_mint: 0,
            to_burn: 0,
        })
    }

    pub(crate) fn enable_mint(mut self, receiver: &'a AccountInfo<'info>) -> Self {
        self.receiver = Some(receiver);
        self
    }

    pub(crate) fn enable_burn(mut self, vault: &'a AccountInfo<'info>) -> Self {
        self.vault = Some(vault);
        self
    }

    pub(crate) fn with_swap_pricing_kind(mut self, kind: SwapPricingKind) -> Self {
        self.base.set_swap_pricing_kind(kind);
        self
    }

    fn transfer(&self) -> TransferUtils<'a, 'info> {
        TransferUtils::new(
            self.token_program.clone(),
            self.store,
            self.market_token.to_account_info(),
        )
    }

    pub(crate) fn base(&self) -> &RevertibleMarket<'a, 'info> {
        &self.base
    }

    pub(crate) fn base_mut(&mut self) -> &mut RevertibleMarket<'a, 'info> {
        &mut self.base
    }

    pub(crate) fn market_token(&self) -> &Account<'info, Mint> {
        self.market_token
    }
}

impl Key for RevertibleLiquidityMarket<'_, '_> {
    fn key(&self) -> Pubkey {
        self.base.key()
    }
}

impl Revision for RevertibleLiquidityMarket<'_, '_> {
    fn rev(&self) -> u64 {
        self.base().rev()
    }
}

impl HasMarketMeta for RevertibleLiquidityMarket<'_, '_> {
    fn market_meta(&self) -> &crate::states::MarketMeta {
        self.base.market_meta()
    }
}

impl Revertible for RevertibleLiquidityMarket<'_, '_> {
    fn commit(self) {
        if self.to_mint != 0 {
            self.transfer()
                .mint_to(self.receiver.expect("mint is not enabled"), self.to_mint)
                .map_err(|err| panic!("mint error: {err}"))
                .unwrap();
        }
        if self.to_burn != 0 {
            self.transfer()
                .burn_from(self.vault.expect("burn is not enabled"), self.to_burn)
                .map_err(|err| panic!("burn error: {err}"))
                .unwrap();
        }
        self.base.commit();
    }
}

impl<'a, 'info> gmsol_model::Bank<Pubkey> for RevertibleLiquidityMarket<'a, 'info> {
    type Num = <RevertibleMarket<'a, 'info> as gmsol_model::Bank<Pubkey>>::Num;

    fn record_transferred_in_by_token<Q: std::borrow::Borrow<Pubkey> + ?Sized>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> gmsol_model::Result<()> {
        self.base.record_transferred_in_by_token(token, amount)
    }

    fn record_transferred_out_by_token<Q: std::borrow::Borrow<Pubkey> + ?Sized>(
        &mut self,
        token: &Q,
        amount: &Self::Num,
    ) -> gmsol_model::Result<()> {
        self.base.record_transferred_out_by_token(token, amount)
    }

    fn balance<Q: std::borrow::Borrow<Pubkey> + ?Sized>(
        &self,
        token: &Q,
    ) -> gmsol_model::Result<Self::Num> {
        self.base.balance(token)
    }
}

impl<'a, 'info> gmsol_model::BaseMarket<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'a, 'info>
{
    type Num = <RevertibleMarket<'a, 'info> as gmsol_model::BaseMarket<
        { constants::MARKET_DECIMALS },
    >>::Num;

    type Signed = <RevertibleMarket<'a, 'info> as gmsol_model::BaseMarket<
        { constants::MARKET_DECIMALS },
    >>::Signed;

    type Pool = <RevertibleMarket<'a, 'info> as gmsol_model::BaseMarket<
        { constants::MARKET_DECIMALS },
    >>::Pool;

    fn liquidity_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.base.liquidity_pool()
    }

    fn claimable_fee_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.base.claimable_fee_pool()
    }

    fn swap_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.base.swap_impact_pool()
    }

    fn open_interest_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.base.open_interest_pool(is_long)
    }

    fn open_interest_in_tokens_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.base.open_interest_in_tokens_pool(is_long)
    }

    fn collateral_sum_pool(&self, is_long: bool) -> gmsol_model::Result<&Self::Pool> {
        self.base.collateral_sum_pool(is_long)
    }

    fn usd_to_amount_divisor(&self) -> Self::Num {
        self.base.usd_to_amount_divisor()
    }

    fn max_pool_amount(&self, is_long_token: bool) -> gmsol_model::Result<Self::Num> {
        self.base.max_pool_amount(is_long_token)
    }

    fn pnl_factor_config(
        &self,
        kind: gmsol_model::PnlFactorKind,
        is_long: bool,
    ) -> gmsol_model::Result<Self::Num> {
        self.base.pnl_factor_config(kind, is_long)
    }

    fn reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        self.base.reserve_factor()
    }

    fn open_interest_reserve_factor(&self) -> gmsol_model::Result<Self::Num> {
        self.base.open_interest_reserve_factor()
    }

    fn max_open_interest(&self, is_long: bool) -> gmsol_model::Result<Self::Num> {
        self.base.max_open_interest(is_long)
    }

    fn ignore_open_interest_for_usage_factor(&self) -> gmsol_model::Result<bool> {
        self.base.ignore_open_interest_for_usage_factor()
    }
}

impl gmsol_model::BaseMarketMut<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn liquidity_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.base.liquidity_pool_mut()
    }

    fn claimable_fee_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.base.claimable_fee_pool_mut()
    }
}

impl gmsol_model::SwapMarket<{ constants::MARKET_DECIMALS }> for RevertibleLiquidityMarket<'_, '_> {
    fn swap_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Self::Num>> {
        self.base.swap_impact_params()
    }

    fn swap_fee_params(&self) -> gmsol_model::Result<gmsol_model::params::FeeParams<Self::Num>> {
        self.base.swap_fee_params()
    }
}

impl gmsol_model::SwapMarketMut<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn swap_impact_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.base.swap_impact_pool_mut()
    }
}

impl gmsol_model::PositionImpactMarket<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn position_impact_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.base.position_impact_pool()
    }

    fn position_impact_params(&self) -> gmsol_model::Result<PriceImpactParams<Self::Num>> {
        self.base.position_impact_params()
    }

    fn position_impact_distribution_params(
        &self,
    ) -> gmsol_model::Result<PositionImpactDistributionParams<Self::Num>> {
        self.base.position_impact_distribution_params()
    }

    fn passed_in_seconds_for_position_impact_distribution(&self) -> gmsol_model::Result<u64> {
        self.base
            .passed_in_seconds_for_position_impact_distribution()
    }
}

impl gmsol_model::PositionImpactMarketMut<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn position_impact_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.base.position_impact_pool_mut()
    }

    fn just_passed_in_seconds_for_position_impact_distribution(
        &mut self,
    ) -> gmsol_model::Result<u64> {
        self.base
            .just_passed_in_seconds_for_position_impact_distribution()
    }
}

impl gmsol_model::BorrowingFeeMarket<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn borrowing_factor_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.base.borrowing_factor_pool()
    }

    fn total_borrowing_pool(&self) -> gmsol_model::Result<&Self::Pool> {
        self.base.total_borrowing_pool()
    }

    fn borrowing_fee_params(
        &self,
    ) -> gmsol_model::Result<gmsol_model::params::fee::BorrowingFeeParams<Self::Num>> {
        self.base.borrowing_fee_params()
    }

    fn passed_in_seconds_for_borrowing(&self) -> gmsol_model::Result<u64> {
        self.base.passed_in_seconds_for_borrowing()
    }

    fn borrowing_fee_kink_model_params(
        &self,
    ) -> gmsol_model::Result<gmsol_model::params::fee::BorrowingFeeKinkModelParams<Self::Num>> {
        self.base.borrowing_fee_kink_model_params()
    }
}

impl gmsol_model::BorrowingFeeMarketMut<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn just_passed_in_seconds_for_borrowing(&mut self) -> gmsol_model::Result<u64> {
        self.base.just_passed_in_seconds_for_borrowing()
    }

    fn borrowing_factor_pool_mut(&mut self) -> gmsol_model::Result<&mut Self::Pool> {
        self.base.borrowing_factor_pool_mut()
    }
}

impl gmsol_model::LiquidityMarket<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn total_supply(&self) -> Self::Num {
        let supply = u128::from(self.market_token.supply);
        supply
            .saturating_add(self.to_mint.into())
            .saturating_sub(self.to_burn.into())
    }

    fn max_pool_value_for_deposit(&self, is_long_token: bool) -> gmsol_model::Result<Self::Num> {
        self.base.market.max_pool_value_for_deposit(is_long_token)
    }
}

impl gmsol_model::LiquidityMarketMut<{ constants::MARKET_DECIMALS }>
    for RevertibleLiquidityMarket<'_, '_>
{
    fn mint(&mut self, amount: &Self::Num) -> gmsol_model::Result<()> {
        let new_mint: u64 = (*amount)
            .try_into()
            .map_err(|_| gmsol_model::Error::Overflow)?;
        let to_mint = self
            .to_mint
            .checked_add(new_mint)
            .ok_or(gmsol_model::Error::Overflow)?;
        // CHECK for overflow.
        self.market_token
            .supply
            .checked_add(to_mint)
            .ok_or(gmsol_model::Error::Overflow)?;
        self.to_mint = to_mint;
        Ok(())
    }

    fn burn(&mut self, amount: &Self::Num) -> gmsol_model::Result<()> {
        let new_burn: u64 = (*amount)
            .try_into()
            .map_err(|_| gmsol_model::Error::Overflow)?;
        let to_burn = self
            .to_burn
            .checked_add(new_burn)
            .ok_or(gmsol_model::Error::Overflow)?;
        // CHECK for underflow.
        self.market_token
            .supply
            .checked_sub(to_burn)
            .ok_or(gmsol_model::Error::Computation(
                "not enough market tokens to burn",
            ))?;
        self.to_burn = to_burn;
        Ok(())
    }
}
