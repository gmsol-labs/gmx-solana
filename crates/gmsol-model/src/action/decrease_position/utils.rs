use num_traits::{CheckedAdd, CheckedSub, Signed, Zero};

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
    T::Signed: CheckedSub + Clone + Ord + UnsignedAbs,
{
    let mut execution_price = index_price.pick_price(is_long).clone();
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

        let adjustment = size_in_usd
            .checked_mul_div_with_signed_numerator(&adjusted_price_impact_value, size_in_tokens)
            .ok_or(crate::Error::Computation(
                "calculating execution price adjustment",
            ))?
            / (size_delta_usd.clone())
                .try_into()
                .map_err(|_| crate::Error::Convert)?;
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
