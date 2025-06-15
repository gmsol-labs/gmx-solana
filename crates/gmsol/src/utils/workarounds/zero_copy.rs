use std::sync::Arc;

use anchor_client::{
    anchor_lang::{AccountDeserialize, Discriminator},
    solana_client::nonblocking::rpc_client::RpcClient,
    solana_sdk::pubkey::Pubkey,
};

/// A workaround to deserialize "zero-copy" account data.
///
/// See [anchort#2689](https://github.com/coral-xyz/anchor/issues/2689) for more information.
pub async fn try_deserailize_zero_copy_account<T>(
    client: &RpcClient,
    pubkey: &Pubkey,
) -> crate::Result<T>
where
    T: anchor_client::anchor_lang::ZeroCopy,
{
    let data = client
        .get_account_data(pubkey)
        .await
        .map_err(anchor_client::ClientError::from)?;

    Ok(gmsol_store::utils::de::try_deserailize(&data)?)
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

impl<T> AccountDeserialize for ZeroCopy<T>
where
    T: anchor_client::anchor_lang::ZeroCopy,
{
    fn try_deserialize(buf: &mut &[u8]) -> anchor_client::anchor_lang::Result<Self> {
        let account = gmsol_store::utils::de::try_deserailize(buf)?;
        Ok(Self(account))
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_client::anchor_lang::Result<Self> {
        let account = gmsol_store::utils::de::try_deserialize_unchecked(buf)?;
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

impl<T> AccountDeserialize for Shared<T>
where
    T: AccountDeserialize,
{
    fn try_deserialize(buf: &mut &[u8]) -> anchor_client::anchor_lang::Result<Self> {
        let account = T::try_deserialize(buf)?;
        Ok(Self(Arc::new(account)))
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_client::anchor_lang::Result<Self> {
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

impl<T> AccountDeserialize for SharedZeroCopy<T>
where
    T: anchor_client::anchor_lang::ZeroCopy,
{
    fn try_deserialize(buf: &mut &[u8]) -> anchor_client::anchor_lang::Result<Self> {
        let account = gmsol_store::utils::de::try_deserailize(buf)?;
        Ok(Self(Arc::new(account)))
    }

    fn try_deserialize_unchecked(buf: &mut &[u8]) -> anchor_client::anchor_lang::Result<Self> {
        let account = gmsol_store::utils::de::try_deserialize_unchecked(buf)?;
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
