use std::ops::AddAssign;

use solana_sdk::{compute_budget::ComputeBudgetInstruction, instruction::Instruction};

/// Compute Budget.
#[derive(Debug, Clone, Copy)]
pub struct ComputeBudget {
    min_priority_lamports: Option<u64>,
    limit_units: u32,
    price_micro_lamports: u64,
}

impl Default for ComputeBudget {
    fn default() -> Self {
        Self {
            min_priority_lamports: Some(Self::DEFAULT_MIN_PRIORITY_LAMPORTS),
            limit_units: 200_000,
            price_micro_lamports: 50_000,
        }
    }
}

impl ComputeBudget {
    const MICRO_LAMPORTS: u64 = 10u64.pow(6);

    /// Minimum priority lamports.
    pub const DEFAULT_MIN_PRIORITY_LAMPORTS: u64 = 10000;

    /// Max compute unit.
    pub const MAX_COMPUTE_UNIT: u32 = 1_400_000;

    /// Set compute units limit.
    #[inline]
    pub fn with_limit(mut self, units: u32) -> Self {
        self.set_limit(units);
        self
    }

    /// Set compute unit price.
    #[inline]
    pub fn with_price(mut self, micro_lamports: u64) -> Self {
        self.set_price(micro_lamports);
        self
    }

    /// Set min priority lamports.
    #[inline]
    pub fn with_min_priority_lamports(mut self, lamports: Option<u64>) -> Self {
        self.set_min_priority_lamports(lamports);
        self
    }

    /// Set min priority lamports.
    #[inline]
    pub fn set_min_priority_lamports(&mut self, lamports: Option<u64>) -> &mut Self {
        self.min_priority_lamports = lamports;
        self
    }

    /// Set compute unit price.
    pub fn set_price(&mut self, micro_lamports: u64) -> &mut Self {
        self.price_micro_lamports = micro_lamports;
        self
    }

    /// Set compute unit limit.
    pub fn set_limit(&mut self, units: u32) -> &mut Self {
        self.limit_units = units;
        self
    }

    fn budget_price(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> u64 {
        let mut price = compute_unit_price_micro_lamports.unwrap_or(self.price_micro_lamports);
        let min_priority_lamports =
            compute_unit_min_priority_lamports.or(self.min_priority_lamports);
        if let Some(min_price) = min_priority_lamports.and_then(|min_lamports| {
            min_lamports
                .checked_mul(Self::MICRO_LAMPORTS)?
                .checked_div(self.budget_units() as u64)
        }) {
            price = price.max(min_price)
        }
        price
    }

    fn budget_units(&self) -> u32 {
        self.limit_units.min(Self::MAX_COMPUTE_UNIT)
    }

    /// Build compute budget instructions.
    pub fn compute_budget_instructions(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> Vec<Instruction> {
        let price = self.budget_price(
            compute_unit_price_micro_lamports,
            compute_unit_min_priority_lamports,
        );
        vec![
            ComputeBudgetInstruction::set_compute_unit_limit(self.budget_units()),
            ComputeBudgetInstruction::set_compute_unit_price(price),
        ]
    }

    /// Get compute unit limit.
    pub fn limit(&self) -> u32 {
        self.limit_units
    }

    /// Get compute unit price in mciro lamports.
    pub fn price(&self) -> u64 {
        self.price_micro_lamports
    }

    /// Estimate priority fee.
    pub fn fee(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
        compute_unit_min_priority_lamports: Option<u64>,
    ) -> u64 {
        self.budget_units() as u64
            * self.budget_price(
                compute_unit_price_micro_lamports,
                compute_unit_min_priority_lamports,
            )
            / Self::MICRO_LAMPORTS
    }
}

impl AddAssign for ComputeBudget {
    fn add_assign(&mut self, rhs: Self) {
        self.limit_units += rhs.limit_units;
        self.price_micro_lamports = self.price_micro_lamports.max(rhs.price_micro_lamports);
        let min_lamports = match (self.min_priority_lamports, rhs.min_priority_lamports) {
            (Some(lamports), None) => Some(lamports),
            (None, Some(lamports)) => Some(lamports),
            (Some(lhs), Some(rhs)) => Some(lhs.max(rhs)),
            (None, None) => None,
        };
        self.min_priority_lamports = min_lamports;
    }
}
