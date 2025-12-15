use gmsol_model::utils::market_token_amount_to_usd;
use solana_sdk::pubkey::Pubkey;

use crate::{
    glv::{GlvModel, GlvStatus},
    market::{caluclator::MarketCalculator, MarketCalculations, Value},
};

/// Performs GLV calculations.
pub trait GlvCalculator: MarketCalculator {
    /// Returns [`GlvModel`] corresponding to the given GLV token.
    fn get_glv_model(&self, glv_token: &Pubkey) -> Option<&GlvModel>;

    /// Calcualtes the market token value in GLV.
    fn get_market_token_value_in_glv(
        &self,
        glv_token: &Pubkey,
        market_token: &Pubkey,
        maximize: bool,
    ) -> crate::Result<u128> {
        Calculator::new(self, glv_token)?.get_market_token_value_in_glv(market_token, maximize)
    }

    /// Calculates the total value of the given GLV.
    fn get_glv_value(&self, glv_token: &Pubkey, maximize: bool) -> crate::Result<u128> {
        Calculator::new(self, glv_token)?.get_glv_value(maximize)
    }

    /// Calculates the underlying value represented by the given amount of a GLV token.
    fn get_glv_token_value(
        &self,
        glv_token: &Pubkey,
        amount: u64,
        maximize: bool,
    ) -> crate::Result<u128> {
        Calculator::new(self, glv_token)?.get_glv_token_value(amount, maximize)
    }

    /// Calculates the maximum sellable value through the given market within the given GLV.
    fn get_max_sellable_glv_value_for_market_token(
        &self,
        glv_token: &Pubkey,
        market_token: &Pubkey,
    ) -> crate::Result<u128> {
        Calculator::new(self, glv_token)?.get_max_sellable_glv_value_for_market_token(market_token)
    }

    /// Calculates the maximum sellable value for the given GLV.
    fn get_max_sellable_glv_value(&self, glv_token: &Pubkey) -> crate::Result<u128> {
        let calculator = Calculator::new(self, glv_token)?;

        let mut value = 0u128;
        for market_token in calculator.glv.market_tokens() {
            value = value
                .checked_add(calculator.get_max_sellable_glv_value_for_market_token(&market_token)?)
                .ok_or_else(|| crate::Error::custom("max sellable value overflow"))?;
        }

        Ok(value)
    }

    /// Calculates the status of the given GLV.
    fn get_glv_status(&self, glv_token: &Pubkey) -> crate::Result<GlvStatus> {
        Calculator::new(self, glv_token)?.get_glv_status()
    }
}

struct Calculator<'a, C: ?Sized> {
    glv: &'a GlvModel,
    context: &'a C,
}

impl<'a, C: GlvCalculator + ?Sized> Calculator<'a, C> {
    fn new(context: &'a C, glv_token: &Pubkey) -> crate::Result<Self> {
        let glv = context.get_glv_model(glv_token).ok_or_else(|| {
            crate::Error::custom(format!("[sim] GLV for GLV token `{glv_token}` not found"))
        })?;

        Ok(Self { glv, context })
    }

    fn get_market_token_value_in_glv(
        &self,
        market_token: &Pubkey,
        maximize: bool,
    ) -> crate::Result<u128> {
        let Self { glv, context } = self;
        let balance = glv
            .market_config(market_token)
            .ok_or_else(|| {
                crate::Error::custom(format!(
                    "[sim] the given GLV does not include the specified market token: {market_token}"
                ))
            })?
            .balance;
        let (market, prices) = context.get_market_model_with_prices(market_token)?;
        let value_for_market =
            gmsol_model::glv::get_glv_value_for_market(&prices, market, balance.into(), maximize)?
                .market_token_value_in_glv;
        Ok(value_for_market)
    }

    fn get_max_sellable_glv_value_for_market_token(
        &self,
        market_token: &Pubkey,
    ) -> crate::Result<u128> {
        let value_in_glv = self.get_market_token_value_in_glv(market_token, false)?;
        let (market, prices) = self
            .context
            .get_market_model_with_prices(market_token)
            .expect("must exist");
        let max_sellable_value = market.max_sellable_value(&prices)?;
        Ok(value_in_glv.min(max_sellable_value))
    }

    fn get_max_sellable_glv_value(&self) -> crate::Result<u128> {
        let mut value = 0u128;
        for market_token in self.glv.market_tokens() {
            value = value
                .checked_add(self.get_max_sellable_glv_value_for_market_token(&market_token)?)
                .ok_or_else(|| crate::Error::custom("max sellable value overflow"))?;
        }

        Ok(value)
    }

    fn get_glv_value(&self, maximize: bool) -> crate::Result<u128> {
        let mut value = 0u128;

        for market_token in self.glv.market_tokens() {
            let value_for_market = self.get_market_token_value_in_glv(&market_token, maximize)?;
            value = value
                .checked_add(value_for_market)
                .ok_or(crate::Error::custom("[sim] GLV value overflow"))?;
        }

        Ok(value)
    }

    fn get_glv_token_value(&self, amount: u64, maximize: bool) -> crate::Result<u128> {
        let glv_value = self.get_glv_value(maximize)?;
        let supply = self.glv.supply();
        let value =
            market_token_amount_to_usd(&(u128::from(amount)), &glv_value, &u128::from(supply))
                .ok_or_else(|| {
                    crate::Error::custom("[sim] failed to convert glv token amount into value")
                })?;
        Ok(value)
    }

    fn get_glv_status(&self) -> crate::Result<GlvStatus> {
        let max_sellable_value = self.get_max_sellable_glv_value()?;
        let max_total_value = self.get_glv_value(true)?;
        let min_total_value = self.get_glv_value(false)?;
        Ok(GlvStatus {
            max_sellable_value,
            total_value: Value {
                min: min_total_value,
                max: max_total_value,
            },
        })
    }
}
