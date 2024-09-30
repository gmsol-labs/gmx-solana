use std::ops::AddAssign;

use anchor_client::solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::Instruction,
};

/// Compute Budget.
#[derive(Debug, Clone, Copy)]
pub struct ComputeBudget {
    limit_units: u32,
    price_micro_lamports: u64,
}

impl Default for ComputeBudget {
    fn default() -> Self {
        Self {
            limit_units: 200_000,
            price_micro_lamports: 50_000,
        }
    }
}

impl ComputeBudget {
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

    /// Build compute budget instructions.
    pub fn compute_budget_instructions(
        &self,
        compute_unit_price_micro_lamports: Option<u64>,
    ) -> Vec<Instruction> {
        vec![
            ComputeBudgetInstruction::set_compute_unit_limit(self.limit_units),
            ComputeBudgetInstruction::set_compute_unit_price(
                compute_unit_price_micro_lamports.unwrap_or(self.price_micro_lamports),
            ),
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
    pub fn fee(&self) -> u64 {
        self.limit_units as u64 * self.price_micro_lamports / 1_000_000
    }
}

impl AddAssign for ComputeBudget {
    fn add_assign(&mut self, rhs: Self) {
        self.limit_units += rhs.limit_units;
        self.price_micro_lamports = self.price_micro_lamports.max(rhs.price_micro_lamports);
    }
}
