use std::sync::Arc;

use anchor_lang::{err, Discriminator};
use gmsol_programs::{
    anchor_lang,
    bytemuck::{self, PodCastError},
};

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
pub fn try_deserialize<T>(data: &[u8]) -> anchor_lang::prelude::Result<T>
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
        let account = try_deserialize(buf)?;
        Ok(Self(account))
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let account = try_deserialize_unchecked(buf)?;
        Ok(Self(account))
    }
}

impl<T> Discriminator for ZeroCopy<T>
where
    T: Discriminator,
{
    const DISCRIMINATOR: &'static [u8] = T::DISCRIMINATOR;
}

impl<T> AsRef<T> for ZeroCopy<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

/// Deserialize a [`ZeroCopy`](anchor_lang::ZeroCopy) structure.
pub fn try_deserialize_zero_copy<T: anchor_lang::ZeroCopy>(
    mut data: &[u8],
) -> crate::Result<ZeroCopy<T>> {
    use anchor_lang::AccountDeserialize;
    Ok(ZeroCopy::<T>::try_deserialize(&mut data)?)
}

/// Deserialize a [`ZeroCopy`](anchor_lang::ZeroCopy) structure from base64.
pub fn try_deserialize_zero_copy_from_base64_with_options<T: anchor_lang::ZeroCopy>(
    data: &str,
    no_discriminator: bool,
) -> crate::Result<ZeroCopy<T>> {
    let mut data = crate::utils::base64::decode_base64(data)?;
    if no_discriminator {
        data = [T::DISCRIMINATOR, &data].concat();
    }
    try_deserialize_zero_copy(&data)
}

/// Deserialize a [`ZeroCopy`](anchor_lang::ZeroCopy) structure from base64.
pub fn try_deserialize_zero_copy_from_base64<T: anchor_lang::ZeroCopy>(
    data: &str,
) -> crate::Result<ZeroCopy<T>> {
    try_deserialize_zero_copy_from_base64_with_options(data, false)
}

/// Workaround for deserializing zero-copy accounts and wrapping the result into Arc.
pub struct SharedZeroCopy<T>(pub Arc<T>);

impl<T> Clone for SharedZeroCopy<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> SharedZeroCopy<T> {
    /// Conver into inner value.
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }
}

impl<T> anchor_lang::AccountDeserialize for SharedZeroCopy<T>
where
    T: anchor_lang::ZeroCopy,
{
    fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let account = try_deserialize(buf)?;
        Ok(Self(Arc::new(account)))
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let account = try_deserialize_unchecked(buf)?;
        Ok(Self(Arc::new(account)))
    }
}

impl<T> Discriminator for SharedZeroCopy<T>
where
    T: Discriminator,
{
    const DISCRIMINATOR: &'static [u8] = T::DISCRIMINATOR;
}

impl<T> AsRef<T> for SharedZeroCopy<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

/// Wrapper for deserializing account into arced type.
pub struct Shared<T>(pub Arc<T>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Shared<T> {
    /// Conver into inner value.
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }
}

impl<T> anchor_lang::AccountDeserialize for Shared<T>
where
    T: anchor_lang::AccountDeserialize,
{
    fn try_deserialize(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let account = T::try_deserialize(buf)?;
        Ok(Self(Arc::new(account)))
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_lang::Result<Self> {
        let account = T::try_deserialize_unchecked(buf)?;
        Ok(Self(Arc::new(account)))
    }
}

impl<T> Discriminator for Shared<T>
where
    T: Discriminator,
{
    const DISCRIMINATOR: &'static [u8] = T::DISCRIMINATOR;
}

impl<T> AsRef<T> for Shared<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}
