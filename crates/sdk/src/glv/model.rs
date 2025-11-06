use std::{ops::Deref, sync::Arc};

use gmsol_programs::gmsol_store::accounts::Glv;
use solana_sdk::pubkey::Pubkey;

use crate::constants;

/// GLV model.
#[derive(Debug, Clone)]
pub struct GlvModel {
    glv: Arc<Glv>,
    supply: u64,
}

impl Deref for GlvModel {
    type Target = Glv;

    fn deref(&self) -> &Self::Target {
        &self.glv
    }
}

impl GlvModel {
    /// Create a new [`GlvModel`].
    pub fn new(glv: Arc<Glv>, supply: u64) -> Self {
        Self { glv, supply }
    }

    /// Get current GLV supply
    pub fn supply(&self) -> u64 {
        self.supply
    }

    /// Deposit to GLV.
    pub fn deposit(
        &mut self,
        market_token: &Pubkey,
        amount: u64,
        received_value: u128,
        glv_value: u128,
    ) -> crate::Result<u64> {
        let current_balance = self
            .glv
            .market_config(market_token)
            .ok_or_else(|| {
                crate::Error::custom(format!("[GLV] `{market_token}` not found in GLV"))
            })?
            .balance;
        let next_balance = current_balance
            .checked_add(amount)
            .ok_or(crate::Error::custom("[GLV] market token balance overflow"))?;

        let glv_amount = gmsol_model::utils::usd_to_market_token_amount(
            received_value,
            glv_value,
            u128::from(self.supply),
            constants::MARKET_USD_TO_AMOUNT_DIVISOR,
        )
        .ok_or(crate::Error::custom(
            "[GLV] failed to calcuate GLV amount to mint",
        ))?
        .try_into()
        .map_err(|_| crate::Error::custom("[GLV] GLV amount to mint overflow"))?;

        let next_supply = self
            .supply
            .checked_add(glv_amount)
            .ok_or(crate::Error::custom("[GLV] GLV token supply overflow"))?;

        Arc::make_mut(&mut self.glv)
            .markets
            .get_mut(market_token)
            .expect("must exist")
            .balance = next_balance;

        self.supply = next_supply;

        Ok(glv_amount)
    }

    /// Withdraw from GLV.
    pub fn withdraw_from_glv(
        &mut self,
        market_token: &Pubkey,
        amount: u64,
        glv_token_amount: u64,
    ) -> crate::Result<()> {
        let current_balance = self
            .glv
            .market_config(market_token)
            .ok_or_else(|| {
                crate::Error::custom(format!("[GLV] `{market_token}` not found in GLV"))
            })?
            .balance;
        let next_balance = current_balance
            .checked_sub(amount)
            .ok_or(crate::Error::custom("[GLV] market token balance underflow"))?;

        let next_supply = self
            .supply
            .checked_sub(glv_token_amount)
            .ok_or(crate::Error::custom("[GLV] GLV token supply underflow"))?;

        Arc::make_mut(&mut self.glv)
            .markets
            .get_mut(market_token)
            .expect("must exist")
            .balance = next_balance;

        self.supply = next_supply;

        Ok(())
    }
}
