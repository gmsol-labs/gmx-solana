use gmsol_programs::gmsol_store::{accounts::Position, types::PositionState};
use gmsol_utils::{market::MarketMeta, token_config::TokenMapAccess};

use crate::utils::{market::MarketDecimals, Amount, Value};

use super::StringPubkey;

/// Serializable version of [`Position`].
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdePosition {
    /// The store address.
    pub store: StringPubkey,
    /// Indicates the side of the position.
    pub is_long: bool,
    /// The owner address.
    pub owner: StringPubkey,
    /// The market token address.
    pub market_token: StringPubkey,
    /// The collateral token address.
    pub collateral_token: StringPubkey,
    /// Position state.
    pub state: SerdePositionState,
}

impl SerdePosition {
    /// Create from [`Position`].
    pub fn from_position(
        position: &Position,
        meta: &MarketMeta,
        token_map: &impl TokenMapAccess,
    ) -> crate::Result<Self> {
        let is_collateral_long_token = meta
            .to_token_side(&position.collateral_token)
            .map_err(crate::Error::custom)?;
        let decimals = MarketDecimals::new(meta, token_map)?;
        Ok(Self {
            store: position.store.into(),
            is_long: position.try_is_long()?,
            owner: position.owner.into(),
            market_token: position.market_token.into(),
            collateral_token: position.collateral_token.into(),
            state: SerdePositionState::from_state(
                &position.state,
                decimals,
                is_collateral_long_token,
            )?,
        })
    }
}

/// Serializable version of [`PositionState`].
#[derive(Debug, Clone)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
pub struct SerdePositionState {
    /// Trade ID.
    pub trade_id: u64,
    /// The time that the position last increased at.
    pub increased_at: i64,
    /// Updated at slot.
    pub updated_at_slot: u64,
    /// The time that the position last decreased at.
    pub decreased_at: i64,
    /// Size in tokens.
    pub size_in_tokens: Amount,
    /// Collateral amount.
    pub collateral_amount: Amount,
    /// Size in USD.
    pub size_in_usd: Value,
    /// Borrowing factor.
    pub borrowing_factor: Value,
    /// Funding fee amount per size.
    pub funding_fee_amount_per_size: Amount,
    /// Claimable funding amount in long token per size.
    pub long_token_claimable_funding_amount_per_size: Amount,
    /// Claimable funding amount in short token per size.
    pub short_token_claimable_funding_amount_per_size: Amount,
}

impl SerdePositionState {
    /// Create from [`PositionState`].
    pub fn from_state(
        state: &PositionState,
        decimals: MarketDecimals,
        is_collateral_long_token: bool,
    ) -> crate::Result<Self> {
        let collateral_token_decimals = if is_collateral_long_token {
            decimals.long_token_decimals
        } else {
            decimals.short_token_decimals
        };
        Ok(Self {
            trade_id: state.trade_id,
            increased_at: state.increased_at,
            updated_at_slot: state.updated_at_slot,
            decreased_at: state.decreased_at,
            size_in_tokens: Amount::from_u128(state.size_in_tokens, decimals.index_token_decimals)?,
            collateral_amount: Amount::from_u128(
                state.collateral_amount,
                collateral_token_decimals,
            )?,
            size_in_usd: Value::from_u128(state.size_in_usd),
            borrowing_factor: Value::from_u128(state.borrowing_factor),
            funding_fee_amount_per_size: unpack_funding_amount_per_size(
                state.funding_fee_amount_per_size,
                collateral_token_decimals,
            )?,
            long_token_claimable_funding_amount_per_size: unpack_funding_amount_per_size(
                state.long_token_claimable_funding_amount_per_size,
                decimals.long_token_decimals,
            )?,
            short_token_claimable_funding_amount_per_size: unpack_funding_amount_per_size(
                state.short_token_claimable_funding_amount_per_size,
                decimals.short_token_decimals,
            )?,
        })
    }
}

fn unpack_funding_amount_per_size(factor: u128, token_decimals: u8) -> crate::Result<Amount> {
    use gmsol_programs::constants::FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT;
    use rust_decimal::{prelude::FromPrimitive, Decimal};

    let adjustment = Decimal::from_i128(FUNDING_AMOUNT_PER_SIZE_ADJUSTMENT as i128).unwrap();
    let mut amount = Amount::from_u128(factor, token_decimals)?;
    amount.0 /= adjustment;
    Ok(amount)
}
