use anchor_lang::prelude::*;
use borsh::BorshSerialize;

use gmsol_model::action::{decrease_position::DecreasePositionSwapType, swap::SwapReport};

use super::Event;

/// Swap executed Event.
#[event]
#[cfg_attr(feature = "debug", derive(Debug))]
pub struct SwapExecuted {
    /// Revision.
    pub rev: u64,
    /// Market token.
    pub market_token: Pubkey,
    /// Report.
    pub report: SwapReport<u128, i128>,
    /// Type.
    pub ty: Option<DecreasePositionSwapType>,
}

impl gmsol_utils::InitSpace for SwapExecuted {
    const INIT_SPACE: usize =
        8 + 32 + SwapReport::<u128, i128>::INIT_SPACE + 1 + DecreasePositionSwapType::INIT_SPACE;
}

impl Event for SwapExecuted {}

impl SwapExecuted {
    /// Create.
    pub fn new(
        rev: u64,
        market_token: Pubkey,
        report: SwapReport<u128, i128>,
        ty: Option<DecreasePositionSwapType>,
    ) -> Self {
        Self {
            rev,
            market_token,
            report,
            ty,
        }
    }
}
