use anchor_lang::prelude::*;

use gmsol_utils::market::{
    MarketConfigFactor, MarketConfigFlag, MarketConfigKey, MAX_MARKET_CONFIG_FACTORS,
};

use crate::{states::market::config::MarketConfigFlagContainer, CoreError, CoreResult};

gmsol_utils::flags!(MarketConfigFactor, MAX_MARKET_CONFIG_FACTORS, u128);

/// Permission store related to market config.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct MarketConfigPermissions {
    /// Market config flags updatable by a [`MARKET_CONFIG_KEEPER`](`gmsol_utils::role::RoleKey::MARKET_CONFIG_KEEPER`).
    updatable_market_config_flags: MarketConfigFlagContainer,
    /// Market config factors updatable by a [`MARKET_CONFIG_KEEPER`](`gmsol_utils::role::RoleKey::MARKET_CONFIG_KEEPER`).
    updatable_market_config_factors: MarketConfigFactorContainer,
}

impl MarketConfigPermissions {
    pub(crate) fn is_flag_updatable(&self, flag: MarketConfigFlag) -> bool {
        self.updatable_market_config_flags.get_flag(flag)
    }

    pub(crate) fn set_flag_updatable(
        &mut self,
        flag: MarketConfigFlag,
        updatable: bool,
    ) -> Result<()> {
        require_neq!(
            self.is_flag_updatable(flag),
            updatable,
            CoreError::PreconditionsAreNotMet
        );
        self.updatable_market_config_flags.set_flag(flag, updatable);
        Ok(())
    }

    fn to_factor(key: MarketConfigKey) -> CoreResult<MarketConfigFactor> {
        key.try_into().map_err(CoreError::from)
    }

    pub(crate) fn is_factor_updatable(&self, key: MarketConfigKey) -> Result<bool> {
        Ok(self
            .updatable_market_config_factors
            .get_flag(Self::to_factor(key).map_err(|err| error!(err))?))
    }

    pub(crate) fn set_factor_updatable(
        &mut self,
        key: MarketConfigKey,
        updatable: bool,
    ) -> Result<()> {
        let factor = Self::to_factor(key).map_err(|err| error!(err))?;

        require_neq!(
            self.updatable_market_config_factors.get_flag(factor),
            updatable,
            CoreError::PreconditionsAreNotMet
        );

        self.updatable_market_config_factors
            .set_flag(factor, updatable);

        Ok(())
    }
}
