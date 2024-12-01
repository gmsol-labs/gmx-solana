use crate::{constants, CoreError};
use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use num_enum::TryFromPrimitive;

use super::{Market, Seed};

/// Position.
#[account(zero_copy)]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct Position {
    /// Bump seed.
    pub bump: u8,
    /// Store.
    pub store: Pubkey,
    /// Position kind (the representation of [`PositionKind`]).
    pub kind: u8,
    /// Padding.
    pub padding_0: [u8; 14],
    /// Owner.
    pub owner: Pubkey,
    /// The market token of the position market.
    pub market_token: Pubkey,
    /// Collateral token.
    pub collateral_token: Pubkey,
    /// Position State.
    pub state: PositionState,
    // /// Reserved.
    // reserved: [u8; 128],
}

impl Default for Position {
    fn default() -> Self {
        use bytemuck::Zeroable;

        Self::zeroed()
    }
}

impl Space for Position {
    #[allow(clippy::identity_op)]
    const INIT_SPACE: usize = std::mem::size_of::<Position>();
}

impl Seed for Position {
    const SEED: &'static [u8] = b"position";
}

impl Position {
    /// Get position kind.
    ///
    /// Note that `Uninitialized` kind will also be returned without error.
    #[inline]
    pub fn kind_unchecked(&self) -> Result<PositionKind> {
        let kind = PositionKind::try_from_primitive(self.kind)?;
        Ok(kind)
    }

    /// Get **initialized** position kind.
    pub fn kind(&self) -> Result<PositionKind> {
        match self.kind_unchecked()? {
            PositionKind::Uninitialized => Err(CoreError::InvalidPosition.into()),
            kind => Ok(kind),
        }
    }

    /// Returns whether the position side is long.
    pub fn try_is_long(&self) -> Result<bool> {
        Ok(matches!(self.kind()?, PositionKind::Long))
    }

    /// Initialize the position state.
    ///
    /// Returns error if
    /// - `kind` is `Unitialized`.
    /// - The kind of the position is not `Uninitialized`.
    pub fn try_init(
        &mut self,
        kind: PositionKind,
        bump: u8,
        store: Pubkey,
        owner: &Pubkey,
        market_token: &Pubkey,
        collateral_token: &Pubkey,
    ) -> Result<()> {
        let PositionKind::Uninitialized = self.kind_unchecked()? else {
            return err!(CoreError::InvalidPosition);
        };
        if matches!(kind, PositionKind::Uninitialized) {
            return err!(CoreError::InvalidPosition);
        }
        self.kind = kind as u8;
        self.bump = bump;
        self.store = store;
        self.padding_0 = [0; 14];
        self.owner = *owner;
        self.market_token = *market_token;
        self.collateral_token = *collateral_token;
        Ok(())
    }

    /// Convert to a type that implements [`Position`](gmsol_model::Position).
    pub fn as_position<'a>(&'a self, market: &'a Market) -> Result<AsPosition<'a>> {
        AsPosition::try_new(self, market)
    }

    pub(crate) fn validate_for_market(&self, market: &Market) -> gmsol_model::Result<()> {
        let meta = market
            .validated_meta(&self.store)
            .map_err(|_| gmsol_model::Error::InvalidPosition("invalid or disabled market"))?;

        if meta.market_token_mint != self.market_token {
            return Err(gmsol_model::Error::InvalidPosition(
                "position's market token does not match the market's",
            ));
        }

        if !meta.is_collateral_token(&self.collateral_token) {
            return Err(gmsol_model::Error::InvalidPosition(
                "invalid collateral token for market",
            ));
        }

        Ok(())
    }
}

impl AsRef<Position> for Position {
    fn as_ref(&self) -> &Position {
        self
    }
}

/// Position State.
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[account(zero_copy)]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct PositionState {
    /// Trade id.
    pub trade_id: u64,
    /// The time that the position last increased at.
    pub increased_at: i64,
    /// Updated at slot.
    pub updated_at_slot: u64,
    /// The time that the position last decreased at.
    pub decreased_at: i64,
    /// Size in tokens.
    pub size_in_tokens: u128,
    /// Collateral amount.
    pub collateral_amount: u128,
    /// Size in usd.
    pub size_in_usd: u128,
    /// Borrowing factor.
    pub borrowing_factor: u128,
    /// Funding fee amount per size.
    pub funding_fee_amount_per_size: u128,
    /// Long token claimable funding amount per size.
    pub long_token_claimable_funding_amount_per_size: u128,
    /// Short token claimable funding amount per size.
    pub short_token_claimable_funding_amount_per_size: u128,
    // /// Reserved.
    // #[cfg_attr(feature = "serde", serde(skip, default = "default_reserved_state"))]
    // reserved: [u8; 128],
}

// #[cfg(feature = "serde")]
// fn default_reserved_state() -> [u8; 128] {
//     [0; 128]
// }

impl gmsol_model::PositionState<{ constants::MARKET_DECIMALS }> for PositionState {
    type Num = u128;

    type Signed = i128;

    fn collateral_amount(&self) -> &Self::Num {
        &self.collateral_amount
    }

    fn size_in_usd(&self) -> &Self::Num {
        &self.size_in_usd
    }

    fn size_in_tokens(&self) -> &Self::Num {
        &self.size_in_tokens
    }

    fn borrowing_factor(&self) -> &Self::Num {
        &self.borrowing_factor
    }

    fn funding_fee_amount_per_size(&self) -> &Self::Num {
        &self.funding_fee_amount_per_size
    }

    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num {
        if is_long_collateral {
            &self.long_token_claimable_funding_amount_per_size
        } else {
            &self.short_token_claimable_funding_amount_per_size
        }
    }
}

impl gmsol_model::PositionStateMut<{ constants::MARKET_DECIMALS }> for PositionState {
    fn collateral_amount_mut(&mut self) -> &mut Self::Num {
        &mut self.collateral_amount
    }

    fn size_in_usd_mut(&mut self) -> &mut Self::Num {
        &mut self.size_in_usd
    }

    fn size_in_tokens_mut(&mut self) -> &mut Self::Num {
        &mut self.size_in_tokens
    }

    fn borrowing_factor_mut(&mut self) -> &mut Self::Num {
        &mut self.borrowing_factor
    }

    fn funding_fee_amount_per_size_mut(&mut self) -> &mut Self::Num {
        &mut self.funding_fee_amount_per_size
    }

    fn claimable_funding_fee_amount_per_size_mut(
        &mut self,
        is_long_collateral: bool,
    ) -> &mut Self::Num {
        if is_long_collateral {
            &mut self.long_token_claimable_funding_amount_per_size
        } else {
            &mut self.short_token_claimable_funding_amount_per_size
        }
    }
}

/// Position Kind.
#[non_exhaustive]
#[repr(u8)]
#[derive(
    Clone,
    Copy,
    num_enum::IntoPrimitive,
    num_enum::TryFromPrimitive,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::Display,
)]
#[strum(serialize_all = "snake_case")]
#[num_enum(error_type(name = CoreError, constructor = CoreError::invalid_position_kind))]
#[cfg_attr(feature = "debug", derive(Debug))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum PositionKind {
    /// Uninitialized.
    Uninitialized,
    /// Long position.
    Long,
    /// Short position.
    Short,
}

/// A helper type that implements the [`Position`](gmsol_model::Position) trait.
pub struct AsPosition<'a> {
    is_long: bool,
    is_collateral_long: bool,
    market: &'a Market,
    position: &'a Position,
}

impl<'a> AsPosition<'a> {
    /// Create from the position and market.
    pub fn try_new(position: &'a Position, market: &'a Market) -> Result<Self> {
        Ok(Self {
            is_long: position.try_is_long()?,
            is_collateral_long: market.meta().to_token_side(&position.collateral_token)?,
            market,
            position,
        })
    }
}

impl<'a> gmsol_model::PositionState<{ constants::MARKET_DECIMALS }> for AsPosition<'a> {
    type Num = u128;

    type Signed = i128;

    fn collateral_amount(&self) -> &Self::Num {
        self.position.state.collateral_amount()
    }

    fn size_in_usd(&self) -> &Self::Num {
        self.position.state.size_in_usd()
    }

    fn size_in_tokens(&self) -> &Self::Num {
        self.position.state.size_in_tokens()
    }

    fn borrowing_factor(&self) -> &Self::Num {
        self.position.state.borrowing_factor()
    }

    fn funding_fee_amount_per_size(&self) -> &Self::Num {
        self.position.state.funding_fee_amount_per_size()
    }

    fn claimable_funding_fee_amount_per_size(&self, is_long_collateral: bool) -> &Self::Num {
        self.position
            .state
            .claimable_funding_fee_amount_per_size(is_long_collateral)
    }
}

impl<'a> gmsol_model::Position<{ constants::MARKET_DECIMALS }> for AsPosition<'a> {
    type Market = Market;

    fn market(&self) -> &Self::Market {
        self.market
    }

    fn is_long(&self) -> bool {
        self.is_long
    }

    fn is_collateral_token_long(&self) -> bool {
        self.is_collateral_long
    }

    fn are_pnl_and_collateral_tokens_the_same(&self) -> bool {
        self.is_long == self.is_collateral_long || self.market.is_pure()
    }

    fn on_validate(&self) -> gmsol_model::Result<()> {
        self.position.validate_for_market(self.market)
    }
}
