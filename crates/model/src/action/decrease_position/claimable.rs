use num_traits::{CheckedAdd, Zero};

/// Claimable collateral amounts.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "anchor-lang",
    derive(anchor_lang::AnchorDeserialize, anchor_lang::AnchorSerialize)
)]
pub struct ClaimableCollateral<T> {
    output_token_amount: T,
    secondary_output_token_amount: T,
}

#[cfg(feature = "gmsol-utils")]
impl<T: gmsol_utils::InitSpace> gmsol_utils::InitSpace for ClaimableCollateral<T> {
    const INIT_SPACE: usize = 2 * T::INIT_SPACE;
}

impl<T> ClaimableCollateral<T> {
    /// Get output token amount.
    pub fn output_token_amount(&self) -> &T {
        &self.output_token_amount
    }

    /// Get secondary output token amount.
    pub fn secondary_output_token_amount(&self) -> &T {
        &self.secondary_output_token_amount
    }

    /// Try to add amount.
    pub fn try_add_amount(&mut self, amount: &T, is_output_token: bool) -> crate::Result<&mut Self>
    where
        T: CheckedAdd,
    {
        let current = if is_output_token {
            &mut self.output_token_amount
        } else {
            &mut self.secondary_output_token_amount
        };
        *current = current.checked_add(amount).ok_or(crate::Error::Overflow)?;
        Ok(self)
    }
}

impl<T: Zero> Default for ClaimableCollateral<T> {
    fn default() -> Self {
        Self {
            output_token_amount: Zero::zero(),
            secondary_output_token_amount: Zero::zero(),
        }
    }
}
