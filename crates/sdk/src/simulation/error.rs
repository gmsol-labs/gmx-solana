//! Standardized simulation errors.

use gmsol_programs::constants;
use solana_sdk::pubkey::Pubkey;

/// A stable code identifying the kind of a simulation failure.
///
/// The codes and their serialized names are stable: existing variants will
/// not be renamed or removed, but new variants may be added over time, so
/// downstream matching should always keep a fallback branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(serde, serde(rename_all = "snake_case"))]
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(into_wasm_abi, from_wasm_abi))]
#[non_exhaustive]
pub enum SimulationErrorCode {
    /// A required market is missing from the simulator.
    MarketNotFound,
    /// A required token is missing from the simulator.
    TokenNotFound,
    /// Prices for a required token or market are missing or not ready.
    PricesNotReady,
    /// The provided swap path is invalid.
    InvalidSwapPath,
    /// A required virtual inventory is missing.
    MissingVirtualInventory,
    /// A virtual inventory is provided but not expected.
    UnexpectedVirtualInventory,
    /// The provided arguments are invalid.
    InvalidArgument,
    /// The provided prices are invalid.
    InvalidPrices,
    /// The deposit is empty.
    EmptyDeposit,
    /// The withdrawal is empty.
    EmptyWithdrawal,
    /// The swap is empty.
    EmptySwap,
    /// The shift is empty.
    EmptyShift,
    /// The GLV deposit is empty.
    EmptyGlvDeposit,
    /// The GLV withdrawal is empty.
    EmptyGlvWithdrawal,
    /// The reserve is insufficient.
    InsufficientReserve,
    /// A PnL factor is exceeded.
    PnlFactorExceeded,
    /// The max pool amount is exceeded.
    MaxPoolAmountExceeded,
    /// The max pool value is exceeded.
    MaxPoolValueExceeded,
    /// The max open interest is exceeded.
    MaxOpenInterestExceeded,
    /// The funds are insufficient to pay for the costs.
    InsufficientFundsToPayForCosts,
    /// The position state is invalid.
    InvalidPosition,
    /// The position is liquidatable.
    Liquidatable,
    /// A numeric computation failed.
    Computation,
    /// The simulation input or state is invalid.
    InvalidSimulationInput,
    /// An unclassified error.
    Unknown,
}

impl SimulationErrorCode {
    /// Returns the stable serialized name of this code.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MarketNotFound => "market_not_found",
            Self::TokenNotFound => "token_not_found",
            Self::PricesNotReady => "prices_not_ready",
            Self::InvalidSwapPath => "invalid_swap_path",
            Self::MissingVirtualInventory => "missing_virtual_inventory",
            Self::UnexpectedVirtualInventory => "unexpected_virtual_inventory",
            Self::InvalidArgument => "invalid_argument",
            Self::InvalidPrices => "invalid_prices",
            Self::EmptyDeposit => "empty_deposit",
            Self::EmptyWithdrawal => "empty_withdrawal",
            Self::EmptySwap => "empty_swap",
            Self::EmptyShift => "empty_shift",
            Self::EmptyGlvDeposit => "empty_glv_deposit",
            Self::EmptyGlvWithdrawal => "empty_glv_withdrawal",
            Self::InsufficientReserve => "insufficient_reserve",
            Self::PnlFactorExceeded => "pnl_factor_exceeded",
            Self::MaxPoolAmountExceeded => "max_pool_amount_exceeded",
            Self::MaxPoolValueExceeded => "max_pool_value_exceeded",
            Self::MaxOpenInterestExceeded => "max_open_interest_exceeded",
            Self::InsufficientFundsToPayForCosts => "insufficient_funds_to_pay_for_costs",
            Self::InvalidPosition => "invalid_position",
            Self::Liquidatable => "liquidatable",
            Self::Computation => "computation",
            Self::InvalidSimulationInput => "invalid_simulation_input",
            Self::Unknown => "unknown",
        }
    }

    /// Returns the stable human readable message for this code.
    pub fn message(&self) -> &'static str {
        match self {
            Self::MarketNotFound => "market not found in the simulator",
            Self::TokenNotFound => "token not found in the simulator",
            Self::PricesNotReady => "prices are not ready in the simulator",
            Self::InvalidSwapPath => "invalid swap path",
            Self::MissingVirtualInventory => "a required virtual inventory is missing",
            Self::UnexpectedVirtualInventory => "an unexpected virtual inventory is provided",
            Self::InvalidArgument => "invalid argument",
            Self::InvalidPrices => "invalid prices",
            Self::EmptyDeposit => "empty deposit",
            Self::EmptyWithdrawal => "empty withdrawal",
            Self::EmptySwap => "empty swap",
            Self::EmptyShift => "empty shift",
            Self::EmptyGlvDeposit => "empty GLV deposit",
            Self::EmptyGlvWithdrawal => "empty GLV withdrawal",
            Self::InsufficientReserve => "insufficient reserve",
            Self::PnlFactorExceeded => "pnl factor exceeded",
            Self::MaxPoolAmountExceeded => "max pool amount exceeded",
            Self::MaxPoolValueExceeded => "max pool value exceeded",
            Self::MaxOpenInterestExceeded => "max open interest exceeded",
            Self::InsufficientFundsToPayForCosts => "insufficient funds to pay for costs",
            Self::InvalidPosition => "invalid position state",
            Self::Liquidatable => "the position is liquidatable",
            Self::Computation => "computation error",
            Self::InvalidSimulationInput => "invalid simulation input or state",
            Self::Unknown => "simulation failed",
        }
    }
}

impl std::fmt::Display for SimulationErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A standardized simulation error.
///
/// Contains a stable [`code`](Self::code) for programmatic handling, a stable
/// [`message`](Self::message) suitable for display, and free-form
/// [`details`](Self::details) describing the exact failure.
#[derive(Debug, Clone, thiserror::Error)]
#[cfg_attr(serde, derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(js, derive(tsify_next::Tsify))]
#[cfg_attr(js, tsify(into_wasm_abi, from_wasm_abi))]
#[error("{details}")]
pub struct SimulationError {
    /// The stable error code.
    pub code: SimulationErrorCode,
    /// The stable message corresponding to the code.
    pub message: String,
    /// Details describing the exact failure.
    pub details: String,
}

impl SimulationError {
    /// Creates a new [`SimulationError`] with the given code and details.
    pub fn new(code: SimulationErrorCode, details: impl ToString) -> Self {
        Self {
            code,
            message: code.message().to_string(),
            details: details.to_string(),
        }
    }

    pub(crate) fn market_not_found(market_token: &Pubkey) -> Self {
        Self::new(
            SimulationErrorCode::MarketNotFound,
            format!("[sim] market `{market_token}` not found in the simulator"),
        )
    }

    pub(crate) fn token_not_found(token: &Pubkey) -> Self {
        Self::new(
            SimulationErrorCode::TokenNotFound,
            format!("[sim] token `{token}` is not found in the simulator"),
        )
    }

    pub(crate) fn prices_not_ready_for_market(market_token: &Pubkey) -> Self {
        Self::new(
            SimulationErrorCode::PricesNotReady,
            format!("[sim] prices for market `{market_token}` are not ready in the simulator"),
        )
    }

    pub(crate) fn price_not_ready(token: &Pubkey) -> Self {
        Self::new(
            SimulationErrorCode::PricesNotReady,
            format!("[sim] price for {token} is not ready"),
        )
    }

    pub(crate) fn invalid_swap_path(details: impl ToString) -> Self {
        Self::new(SimulationErrorCode::InvalidSwapPath, details)
    }
}

impl From<crate::Error> for SimulationError {
    fn from(error: crate::Error) -> Self {
        match error {
            crate::Error::Simulation(error) => error,
            crate::Error::Model(ref err) => Self::new(classify_model_error(err), &error),
            crate::Error::Custom(ref msg)
                if msg.starts_with("[sim]") || msg.starts_with("[swap]") =>
            {
                Self::new(SimulationErrorCode::InvalidSimulationInput, &error)
            }
            error => Self::new(SimulationErrorCode::Unknown, &error),
        }
    }
}

fn classify_model_error(error: &gmsol_model::Error) -> SimulationErrorCode {
    use gmsol_model::Error as ModelError;
    use SimulationErrorCode as Code;

    match error {
        ModelError::InvalidArgument(msg) => match *msg {
            constants::VI_FOR_SWAPS_MISSING_ERROR | constants::VI_FOR_POSITIONS_MISSING_ERROR => {
                Code::MissingVirtualInventory
            }
            constants::VI_FOR_SWAPS_UNEXPECTED_ERROR
            | constants::VI_FOR_POSITIONS_UNEXPECTED_ERROR => Code::UnexpectedVirtualInventory,
            _ => Code::InvalidArgument,
        },
        ModelError::EmptyDeposit => Code::EmptyDeposit,
        ModelError::EmptyWithdrawal => Code::EmptyWithdrawal,
        ModelError::EmptySwap => Code::EmptySwap,
        ModelError::InvalidPrices => Code::InvalidPrices,
        ModelError::InsufficientReserve(..)
        | ModelError::InsufficientReserveForOpenInterest(..) => Code::InsufficientReserve,
        ModelError::PnlFactorExceeded(..) => Code::PnlFactorExceeded,
        ModelError::MaxPoolAmountExceeded(_) => Code::MaxPoolAmountExceeded,
        ModelError::MaxPoolValueExceeded(_) => Code::MaxPoolValueExceeded,
        ModelError::MaxOpenInterestExceeded => Code::MaxOpenInterestExceeded,
        ModelError::InsufficientFundsToPayForCosts(_) => Code::InsufficientFundsToPayForCosts,
        ModelError::InvalidPosition(_) => Code::InvalidPosition,
        ModelError::Liquidatable(_) => Code::Liquidatable,
        ModelError::MintReceiverNotSet
        | ModelError::WithdrawalVaultNotSet
        | ModelError::InvalidTokenBalance(..)
        | ModelError::NotLiquidatable => Code::InvalidArgument,
        ModelError::Computation(_)
        | ModelError::PoolComputation(..)
        | ModelError::PowComputation
        | ModelError::Overflow
        | ModelError::DividedByZero
        | ModelError::Convert
        | ModelError::InvalidPoolValue(_)
        | ModelError::BuildParams(_)
        | ModelError::MissingPoolKind(_)
        | ModelError::MissingClockKind(_)
        | ModelError::UnableToGetBorrowingFactorEmptyPoolValue
        | ModelError::UnableToGetFundingFactorEmptyOpenInterest => Code::Computation,
        _ => Code::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_model_errors() {
        let error = SimulationError::from(crate::Error::from(gmsol_model::Error::InvalidArgument(
            constants::VI_FOR_POSITIONS_MISSING_ERROR,
        )));
        assert_eq!(error.code, SimulationErrorCode::MissingVirtualInventory);
        assert_eq!(
            error.message,
            SimulationErrorCode::MissingVirtualInventory.message()
        );
        assert!(error.details.contains("virtual inventory for positions"));

        let error = SimulationError::from(crate::Error::from(gmsol_model::Error::InvalidPrices));
        assert_eq!(error.code, SimulationErrorCode::InvalidPrices);

        let error = SimulationError::from(crate::Error::from(
            gmsol_model::Error::MaxOpenInterestExceeded,
        ));
        assert_eq!(error.code, SimulationErrorCode::MaxOpenInterestExceeded);
    }

    #[test]
    fn classify_custom_errors() {
        let error = SimulationError::from(crate::Error::custom("[sim] trigger price is required"));
        assert_eq!(error.code, SimulationErrorCode::InvalidSimulationInput);

        let error = SimulationError::from(crate::Error::custom("some other error"));
        assert_eq!(error.code, SimulationErrorCode::Unknown);
    }

    #[test]
    fn typed_errors_pass_through() {
        let market_token = Pubkey::new_unique();
        let error = SimulationError::from(crate::Error::from(SimulationError::market_not_found(
            &market_token,
        )));
        assert_eq!(error.code, SimulationErrorCode::MarketNotFound);
        assert!(error.details.contains(&market_token.to_string()));
    }

    #[cfg(serde)]
    #[test]
    fn code_serialization_matches_as_str() {
        for code in [
            SimulationErrorCode::MarketNotFound,
            SimulationErrorCode::EmptyGlvWithdrawal,
            SimulationErrorCode::Unknown,
        ] {
            let serialized = serde_json::to_value(code).unwrap();
            assert_eq!(
                serialized,
                serde_json::Value::String(code.as_str().to_string())
            );
        }
    }
}
