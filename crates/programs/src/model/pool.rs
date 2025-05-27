use crate::gmsol_store::types::Pool;

impl gmsol_model::Balance for Pool {
    type Num = u128;

    type Signed = i128;

    /// Get the long token amount.
    fn long_amount(&self) -> gmsol_model::Result<Self::Num> {
        if self.is_pure() {
            debug_assert_eq!(
                self.short_token_amount, 0,
                "short token amount must be zero"
            );
            // For pure pools, we must ensure that the long token amount
            // plus the short token amount equals the total token amount.
            // Therefore, we use `div_ceil` for the long token amount
            // and `div` for the short token amount.
            Ok(self.long_token_amount.div_ceil(2))
        } else {
            Ok(self.long_token_amount)
        }
    }

    /// Get the short token amount.
    fn short_amount(&self) -> gmsol_model::Result<Self::Num> {
        if self.is_pure() {
            debug_assert_eq!(
                self.short_token_amount, 0,
                "short token amount must be zero"
            );
            Ok(self.long_token_amount / 2)
        } else {
            Ok(self.short_token_amount)
        }
    }
}

impl gmsol_model::Pool for Pool {
    fn apply_delta_to_long_amount(&mut self, delta: &Self::Signed) -> gmsol_model::Result<()> {
        self.long_token_amount = self.long_token_amount.checked_add_signed(*delta).ok_or(
            gmsol_model::Error::Computation("apply delta to long amount"),
        )?;
        Ok(())
    }

    fn apply_delta_to_short_amount(&mut self, delta: &Self::Signed) -> gmsol_model::Result<()> {
        let amount = if self.is_pure() {
            &mut self.long_token_amount
        } else {
            &mut self.short_token_amount
        };
        *amount = amount
            .checked_add_signed(*delta)
            .ok_or(gmsol_model::Error::Computation(
                "apply delta to short amount",
            ))?;
        Ok(())
    }

    fn checked_apply_delta(
        &self,
        delta: gmsol_model::Delta<&Self::Signed>,
    ) -> gmsol_model::Result<Self> {
        let mut ans = *self;
        if let Some(amount) = delta.long() {
            ans.apply_delta_to_long_amount(amount)?;
        }
        if let Some(amount) = delta.short() {
            ans.apply_delta_to_short_amount(amount)?;
        }
        Ok(ans)
    }
}
