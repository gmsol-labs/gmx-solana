use crate::Error;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "js")]
use tsify_next::Tsify;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "js", derive(Tsify))]
#[cfg_attr(feature = "js", tsify(into_wasm_abi, from_wasm_abi))]
pub struct SimulationError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "js", derive(Tsify))]
#[cfg_attr(feature = "js", tsify(into_wasm_abi, from_wasm_abi))]
pub enum SimulationErrorCode {
    Unknown,
    MarketNotFound,
    PricesNotReady,
    PriceNotReady,
    InvalidSwapPath,
    TriggerPriceRequired,
    EmptyDeposit,
    EmptyWithdrawal,
    EmptyShift,
    ShiftImpossible,
    InsufficientOutputAmount,
}

impl SimulationErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "SIM_UNKNOWN",
            Self::MarketNotFound => "SIM_MARKET_NOT_FOUND",
            Self::PricesNotReady => "SIM_PRICES_NOT_READY",
            Self::PriceNotReady => "SIM_PRICE_NOT_READY",
            Self::InvalidSwapPath => "SIM_INVALID_SWAP_PATH",
            Self::TriggerPriceRequired => "SIM_TRIGGER_PRICE_REQUIRED",
            Self::EmptyDeposit => "SIM_EMPTY_DEPOSIT",
            Self::EmptyWithdrawal => "SIM_EMPTY_WITHDRAWAL",
            Self::EmptyShift => "SIM_EMPTY_SHIFT",
            Self::ShiftImpossible => "SIM_SHIFT_IMPOSSIBLE",
            Self::InsufficientOutputAmount => "SIM_INSUFFICIENT_OUTPUT_AMOUNT",
        }
    }

    pub fn default_message(&self) -> &'static str {
        match self {
            Self::Unknown => "simulation failed",
            Self::MarketNotFound => "market not found in simulator",
            Self::PricesNotReady => "required prices are not ready in simulator",
            Self::PriceNotReady => "required price is not ready in simulator",
            Self::InvalidSwapPath => "invalid swap path",
            Self::TriggerPriceRequired => "trigger price is required",
            Self::EmptyDeposit => "empty deposit",
            Self::EmptyWithdrawal => "empty withdrawal",
            Self::EmptyShift => "empty shift",
            Self::ShiftImpossible => "shift is impossible",
            Self::InsufficientOutputAmount => "insufficient output amount",
        }
    }
}

impl SimulationError {
    pub fn new(code: SimulationErrorCode, details: Option<String>) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: code.default_message().to_string(),
            details,
        }
    }
}

pub(crate) fn sim_error(_code: SimulationErrorCode, details: String) -> Error {
    #[cfg(nightly_simulation_errors)]
    {
        Error::Simulation(SimulationError::new(_code, Some(details)))
    }

    #[cfg(not(nightly_simulation_errors))]
    {
        Error::custom(details)
    }
}
