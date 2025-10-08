use anchor_lang::Event;

/// Decode anchor event with options.
pub fn decode_anchor_event_with_options<T: Event>(
    mut data: &[u8],
    no_discriminator: bool,
) -> crate::Result<T> {
    if !no_discriminator {
        let len = T::DISCRIMINATOR.len();
        if data.len() < len {
            return Err(crate::Error::custom(
                "invalid event: the data size is too small",
            ));
        }
        let (disc, event_data) = data.split_at(len);
        if disc != T::DISCRIMINATOR {
            return Err(crate::Error::custom(
                "invalid event: discriminator does not match",
            ));
        }
        data = event_data;
    }

    T::try_from_slice(data).map_err(crate::Error::custom)
}
