use anchor_lang::prelude::*;
use gmsol_utils::price::Decimal;

use crate::{utils::pubkey::to_bytes, CoreError};

/// Zero-copy price structure for storing min max prices.
#[zero_copy]
#[cfg_attr(feature = "debug", derive(derive_more::Debug))]
pub struct SmallPrices {
    decimal_multiplier: u8,
    flags: u8,
    #[cfg_attr(feature = "debug", debug(skip))]
    padding_0: [u8; 2],
    min: u32,
    max: u32,
}

impl Default for SmallPrices {
    fn default() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

impl SmallPrices {
    const SYNTHETIC_FLAGS: u8 = u8::MAX;

    /// Get min price.
    pub fn min(&self) -> Decimal {
        Decimal {
            value: self.min,
            decimal_multiplier: self.decimal_multiplier,
        }
    }

    /// Get max price.
    pub fn max(&self) -> Decimal {
        Decimal {
            value: self.max,
            decimal_multiplier: self.decimal_multiplier,
        }
    }

    pub(crate) fn from_price(price: &gmsol_utils::Price, is_synthetic: bool) -> Result<Self> {
        // Validate price data.
        require_eq!(
            price.min.decimal_multiplier,
            price.max.decimal_multiplier,
            CoreError::InvalidArgument
        );
        require_neq!(price.min.value, 0, CoreError::InvalidArgument);
        require_gte!(price.max.value, price.min.value, CoreError::InvalidArgument);

        let flags = if is_synthetic {
            Self::SYNTHETIC_FLAGS
        } else {
            0
        };

        Ok(SmallPrices {
            decimal_multiplier: price.min.decimal_multiplier,
            flags,
            padding_0: [0; 2],
            min: price.min.value,
            max: price.max.value,
        })
    }

    /// Returns whether the token is synthetic.
    pub fn is_synthetic(&self) -> bool {
        self.flags == Self::SYNTHETIC_FLAGS
    }

    /// Convert to [`Price`](gmsol_utils::Price).
    pub fn to_price(&self) -> Result<gmsol_utils::Price> {
        Ok(gmsol_utils::Price {
            min: self.min(),
            max: self.max(),
        })
    }
}

const MAX_TOKENS: usize = 512;

gmsol_utils::fixed_map!(PriceMap, Pubkey, to_bytes, SmallPrices, MAX_TOKENS, 0);

impl PriceMap {
    /// Max tokens.
    pub const MAX_TOKENS: usize = MAX_TOKENS;

    pub(super) fn set(
        &mut self,
        token: &Pubkey,
        price: gmsol_utils::Price,
        is_synthetic: bool,
    ) -> Result<()> {
        self.insert(token, SmallPrices::from_price(&price, is_synthetic)?);
        Ok(())
    }
}
