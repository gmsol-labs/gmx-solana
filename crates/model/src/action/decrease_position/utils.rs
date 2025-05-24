use num_traits::{CheckedAdd, CheckedDiv, CheckedSub, Signed, Zero};

use crate::{
    num::{MulDiv, UnsignedAbs},
    price::Price,
};

pub(super) fn get_execution_price_for_decrease<T>(
    index_price: &Price<T>,
    size_in_usd: &T,
    size_in_tokens: &T,
    size_delta_usd: &T,
    price_impact_value: &T::Signed,
    acceptable_price: Option<&T>,
    is_long: bool,
) -> crate::Result<T>
where
    T: Clone + MulDiv + Ord + CheckedAdd + CheckedSub,
    T::Signed: CheckedSub + Clone + Ord + UnsignedAbs + CheckedDiv,
{
    let mut execution_price = index_price.pick_price(!is_long).clone();
    if !size_delta_usd.is_zero() && !size_in_tokens.is_zero() {
        let adjusted_price_impact_value = if is_long {
            price_impact_value.clone()
        } else {
            T::Signed::zero()
                .checked_sub(price_impact_value)
                .ok_or(crate::Error::Computation("price impact too large"))?
        };

        if adjusted_price_impact_value.is_negative()
            && adjusted_price_impact_value.unsigned_abs() > *size_delta_usd
        {
            return Err(crate::Error::Computation(
                "price impact larger than order size",
            ));
        }

        // Since the decimals of the USD value are often greater than those of `size_in_tokens`,
        // we must compute the `adjustment` in the following sequence. Furthermore, since the
        // `price_impact_value` is comparable in magnitude to `size_delta_usd` and is capped,
        // overflow is unlikely in the `mul_div` part.
        //
        // In practice, since:
        //
        // size_in_usd * abs(price_impact_value) / size_delta_usd
        //     <= size_in_usd * (max_position_impact_factor * size_delta_usd / UNIT) / size_delta_usd
        //     == size_in_usd * max_position_impact_factor / UNIT
        //
        // To ensure that no overflow occurs, we need:
        //
        // Unsigned::MAX * max_position_impact_factor / UNIT <= Signed::MAX
        //
        // Which simplifies to:
        //
        // max_position_impact_factor <= (Signed::MAX / Unsigned::MAX) * UNIT
        //
        // In other words, as long as `max_position_impact_factor` doesn't exceed 50%.
        //
        // Otherwise, if we use the order `size_in_usd.mul_div(adjusted_price_impact_value, size_in_tokens) / size_in_usd`,
        // the `mul_div` operation is likely to overflow. For example (Unsigned = u128, Signed = i128, DECIMALS = 20):
        //
        // Assume that `size_in_usd = 6250 * 10^20` ($6250) and `size_in_tokens = 67774` (0.067774 BTC, decimals = 6). When the user
        // close it with `size_delta_usd = 6250 * 10^20` causing a `price_impact_value = 3.90625 * 10^20` (the "factor" is only
        // 0.0625%), an overflow will occur in the `mul_div` step:
        //
        // size_in_usd * price_impact_value / size_in_tokens >= 3.60 * 10^39 > i128::MAX
        //
        let signed_size_in_tokens = size_in_tokens.to_signed()?;
        let adjustment = size_in_usd
            .checked_mul_div_with_signed_numerator(&adjusted_price_impact_value, size_delta_usd)
            .ok_or(crate::Error::Computation(
                "calculating execution price adjustment",
            ))?
            .checked_div(&signed_size_in_tokens)
            .ok_or(crate::Error::Computation("calculating adjustment"))?;
        execution_price = execution_price
            .checked_add_with_signed(&adjustment)
            .ok_or(crate::Error::Computation("adjusting execution price"))?;
    }
    let Some(acceptable_price) = acceptable_price else {
        return Ok(execution_price);
    };
    if (is_long && execution_price >= *acceptable_price)
        || (!is_long && execution_price <= *acceptable_price)
    {
        Ok(execution_price)
    } else {
        Err(crate::Error::InvalidArgument(
            "order not fulfillable at acceptable price",
        ))
    }
}
