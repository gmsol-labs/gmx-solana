use std::{future::Future, ops::Deref};

use anchor_client::{
    solana_client::rpc_config::RpcAccountInfoConfig,
    solana_sdk::{
        account::ReadableAccount,
        address_lookup_table::{self, state::AddressLookupTable, AddressLookupTableAccount},
        pubkey::Pubkey,
        signer::Signer,
    },
};
use solana_account_decoder::UiAccountEncoding;

use crate::utils::{
    rpc::accounts::get_account_with_context, RpcBuilder, TransactionBuilder, WithSlot,
};

/// Address Lookup Table operations.
pub trait AddressLookupTableOps<C> {
    /// Fetch address lookup table with the given config.
    fn alt_with_config(
        &self,
        address: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> impl Future<Output = crate::Result<WithSlot<Option<AddressLookupTableAccount>>>>;

    /// Fetch address lookup table.
    fn alt(
        &self,
        address: &Pubkey,
    ) -> impl Future<Output = crate::Result<Option<AddressLookupTableAccount>>> {
        async {
            Ok(self
                .alt_with_config(
                    address,
                    RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        ..Default::default()
                    },
                )
                .await?
                .into_value())
        }
    }

    /// Create a [`RpcBuilder`] to create address lookup table.
    fn create_alt(&self) -> impl Future<Output = crate::Result<(RpcBuilder<C>, Pubkey)>>;

    /// Create a [`TransactionBuilder`] to extend the given address lookup table with new addresses.
    fn extend_alt(
        &self,
        alt: &Pubkey,
        new_addresses: Vec<Pubkey>,
        chunk_size: Option<usize>,
    ) -> crate::Result<TransactionBuilder<C>>;
    /// Create a [`RpcBuilder`] to deactivate the given address lookup table
    fn deactivate_alt(&self, alt: &Pubkey) -> RpcBuilder<C>;

    /// Create a [`RpcBuilder`] to close the given address lookup table
    fn close_alt(&self, alt: &Pubkey) -> RpcBuilder<C>;
}

impl<C: Deref<Target = impl Signer> + Clone> AddressLookupTableOps<C> for crate::Client<C> {
    async fn alt_with_config(
        &self,
        address: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> crate::Result<WithSlot<Option<AddressLookupTableAccount>>> {
        let client = self.store_program().solana_rpc();
        let account: WithSlot<_> = get_account_with_context(&client, address, config)
            .await?
            .into();
        account
            .map(|a| {
                a.map(|account| {
                    let table = AddressLookupTable::deserialize(account.data())
                        .map_err(crate::Error::invalid_argument)?;
                    Ok(AddressLookupTableAccount {
                        key: *address,
                        addresses: table.addresses.iter().copied().collect(),
                    })
                })
                .transpose()
            })
            .transpose()
    }

    async fn create_alt(&self) -> crate::Result<(RpcBuilder<C>, Pubkey)> {
        let slot = self.get_slot(None).await?;
        let payer = self.payer();
        let (ix, address) =
            address_lookup_table::instruction::create_lookup_table(payer, payer, slot);
        let rpc = self
            .store_rpc()
            .program(address_lookup_table::program::ID)
            .pre_instruction(ix);

        Ok((rpc, address))
    }

    fn extend_alt(
        &self,
        alt: &Pubkey,
        new_addresses: Vec<Pubkey>,
        chunk_size: Option<usize>,
    ) -> crate::Result<TransactionBuilder<C>> {
        let mut tx = self.transaction();
        let payer = self.payer();

        let chunk_size = chunk_size.unwrap_or(10);
        for new_addresses in new_addresses.chunks(chunk_size) {
            let ix = address_lookup_table::instruction::extend_lookup_table(
                *alt,
                payer,
                Some(payer),
                new_addresses.to_owned(),
            );
            let rpc = self
                .store_rpc()
                .program(address_lookup_table::program::ID)
                .pre_instruction(ix);
            tx.try_push(rpc)?;
        }
        Ok(tx)
    }

    fn deactivate_alt(&self, alt: &Pubkey) -> RpcBuilder<C> {
        let payer = self.payer();
        let ix = address_lookup_table::instruction::deactivate_lookup_table(*alt, payer);
        self.store_rpc()
            .program(address_lookup_table::program::ID)
            .pre_instruction(ix)
    }

    fn close_alt(&self, alt: &Pubkey) -> RpcBuilder<C> {
        let payer = self.payer();
        let ix = address_lookup_table::instruction::close_lookup_table(*alt, payer, payer);
        self.store_rpc()
            .program(address_lookup_table::program::ID)
            .pre_instruction(ix)
    }
}
