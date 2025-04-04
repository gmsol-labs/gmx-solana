use anchor_lang::{err, Discriminator};
use gmsol_programs::{
    anchor_lang,
    bytemuck::{self, PodCastError},
};

/// Check discriminator.
pub fn check_discriminator<T: Discriminator>(data: &[u8]) -> anchor_lang::prelude::Result<()> {
    use anchor_lang::error::ErrorCode;

    let disc = T::discriminator();
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
    try_deserailize_unchecked(data)
}

/// A workaround to deserialize "zero-copy" account data.
///
/// See [anchort#2689](https://github.com/coral-xyz/anchor/issues/2689) for more information.
pub fn try_deserailize_unchecked<T>(data: &[u8]) -> anchor_lang::prelude::Result<T>
where
    T: anchor_lang::ZeroCopy,
{
    use anchor_lang::{error, error::ErrorCode};
    let end = std::mem::size_of::<T>() + 8;
    if data.len() < end {
        return err!(ErrorCode::AccountDidNotDeserialize);
    }
    let data_without_discriminator = &data[8..end];

    match bytemuck::try_from_bytes(data_without_discriminator) {
        Ok(data) => Ok(*data),
        Err(PodCastError::TargetAlignmentGreaterAndInputNotAligned) => {
            bytemuck::try_pod_read_unaligned(data_without_discriminator)
                .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))
        }
        Err(_) => Err(error!(ErrorCode::AccountDidNotDeserialize)),
    }
}

/// Workaround for deserializing zero-copy accounts.
#[derive(Debug, Clone, Copy)]
pub struct ZeroCopy<T>(pub T);

impl<T> ZeroCopy<T> {
    /// Conver into inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> anchor_lang::AccountDeserialize for ZeroCopy<T>
where
    T: anchor_lang::ZeroCopy,
{
    fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let account = try_deserailize(buf)?;
        Ok(Self(account))
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let account = try_deserailize_unchecked(buf)?;
        Ok(Self(account))
    }
}

impl<T> Discriminator for ZeroCopy<T>
where
    T: Discriminator,
{
    const DISCRIMINATOR: [u8; 8] = T::DISCRIMINATOR;
}

impl<T> AsRef<T> for ZeroCopy<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}
