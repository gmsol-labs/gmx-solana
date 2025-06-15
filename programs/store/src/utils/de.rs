use std::borrow::Cow;

use anchor_lang::{err, Discriminator};

/// Check discriminator.
pub fn check_discriminator<T: Discriminator>(data: &[u8]) -> anchor_lang::prelude::Result<()> {
    use anchor_lang::error::ErrorCode;

    let disc = T::DISCRIMINATOR;
    if data.len() < disc.len() {
        return err!(ErrorCode::AccountDiscriminatorNotFound);
    }
    let given_disc = &data[..8];
    if disc != given_disc {
        return err!(ErrorCode::AccountDiscriminatorMismatch);
    }
    Ok(())
}

/// A workaround to deserialize "zero-copy" account data.
///
/// See [anchort#2689](https://github.com/coral-xyz/anchor/issues/2689) for more information.
pub fn try_deserailize<T>(data: &[u8]) -> anchor_lang::prelude::Result<T>
where
    T: anchor_lang::ZeroCopy,
{
    check_discriminator::<T>(data)?;
    try_deserialize_unchecked(data)
}

/// A workaround to deserialize "zero-copy" account data.
///
/// See [anchort#2689](https://github.com/coral-xyz/anchor/issues/2689) for more information.
pub fn try_deserialize_unchecked<T>(data: &[u8]) -> anchor_lang::prelude::Result<T>
where
    T: anchor_lang::ZeroCopy,
{
    use anchor_lang::{error, error::ErrorCode};
    let end = std::mem::size_of::<T>() + 8;
    if data.len() < end {
        return err!(ErrorCode::AccountDidNotDeserialize);
    }
    // Note: We use vec to fix the alignment issue, maybe there is a better way.
    let mut data_without_discriminator = Cow::Borrowed(&data[8..end]);
    Ok(
        *bytemuck::try_from_bytes(data_without_discriminator.to_mut())
            .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))?,
    )
}
