use crate::serde::StringPubkey;

/// Store Program.
pub mod store_program;

/// Instruction builders related to token.
pub mod token;

/// Instruction builders related to order.
pub mod order;

/// Instruction builders related to user.
pub mod user;

/// Instruction builders related to position.
pub mod position;

/// Instruction builders related to market management.
pub mod market;

/// Instruction builders related to market state.
pub mod market_state;

/// Instruction builders related to deposit.
pub mod deposit;

/// Instruction builders related to withdrawal.
pub mod withdrawal;

/// Instruction builders related to shift.
pub mod shift;

/// Instruction builders related to GLV deposit.
pub mod glv_deposit;

/// Instruction builders related to GLV withdrawal.
pub mod glv_withdrawal;

/// Instruction builders related to liquidity provider program.
#[cfg(liquidity_provider)]
pub mod liquidity_provider;

pub(crate) mod utils;

/// Definitions for callback mechanism.
pub mod callback;

/// Nonce Bytes.
pub type NonceBytes = StringPubkey;

pub use self::{
    market::MarketTokenIxBuilder,
    store_program::{StoreProgram, StoreProgramIxBuilder},
    utils::PoolTokenHint,
};
