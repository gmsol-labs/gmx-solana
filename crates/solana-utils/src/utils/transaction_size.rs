use std::{borrow::Borrow, collections::HashSet};

use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

use crate::address_lookup_table::AddressLookupTables;

/// Estimated the size of the result transaction.
///
/// Based on the docs of [Solana Transactions](https://solana.com/docs/core/transactions),
/// and referring to the implementation of `@pythnetwork/solana-utils`.
pub fn transaction_size<T: Borrow<Instruction>>(
    payer: Pubkey,
    ixs: &[T],
    is_versioned_transaction: bool,
    lookup_table: Option<&HashSet<Pubkey>>,
    lookup_table_addresses: usize,
) -> usize {
    use std::collections::HashSet;

    fn get_size_of_compressed_u16(size: usize) -> usize {
        match size {
            0..=127 => 1,
            128..=16383 => 2,
            _ => 3,
        }
    }

    let mut programs = HashSet::<Pubkey>::default();
    let mut accounts = HashSet::<Pubkey>::from([payer]);
    let mut signers = HashSet::<Pubkey>::from([payer]);

    let ixs_size = ixs.iter().fold(0, |size, ix| {
        let ix = ix.borrow();
        programs.insert(ix.program_id);
        accounts.insert(ix.program_id);
        ix.accounts.iter().for_each(|account| {
            accounts.insert(account.pubkey);
            if account.is_signer {
                signers.insert(account.pubkey);
            }
        });
        size + 1
            + get_size_of_compressed_u16(ix.accounts.len())
            + ix.accounts.len()
            + get_size_of_compressed_u16(ix.data.len())
            + ix.data.len()
    });

    let num_of_address_lookups = if let Some(lookup_table) = lookup_table {
        let total_accounts = accounts.len();
        accounts = accounts.difference(lookup_table).copied().collect();
        accounts = accounts
            .union(&signers)
            .chain(programs.iter())
            .copied()
            .collect();
        total_accounts - accounts.len()
    } else {
        0
    };

    let size = get_size_of_compressed_u16(signers.len())
        + signers.len() * 64
        + 3
        + get_size_of_compressed_u16(accounts.len())
        + accounts.len() * 32
        + 32
        + get_size_of_compressed_u16(ixs.len())
        + ixs_size
        + num_of_address_lookups;
    if is_versioned_transaction {
        size + 1 + get_size_of_compressed_u16(0) + lookup_table_addresses * (32 + 2)
    } else {
        size
    }
}

/// Estimated the size of the result transaction with [`AddressLookupTables`].
pub fn transaction_size_with_luts<T: Borrow<Instruction>>(
    payer: Pubkey,
    ixs: &[T],
    is_versioned_transaction: bool,
    luts: Option<&AddressLookupTables>,
) -> usize {
    use std::collections::HashSet;

    fn get_size_of_compressed_u16(size: usize) -> usize {
        match size {
            0..=127 => 1,
            128..=16383 => 2,
            _ => 3,
        }
    }

    let mut programs = HashSet::<Pubkey>::default();
    let mut accounts = HashSet::<Pubkey>::from([payer]);
    let mut signers = HashSet::<Pubkey>::from([payer]);

    let ixs_size = ixs.iter().fold(0, |size, ix| {
        let ix = ix.borrow();
        programs.insert(ix.program_id);
        accounts.insert(ix.program_id);
        ix.accounts.iter().for_each(|account| {
            accounts.insert(account.pubkey);
            if account.is_signer {
                signers.insert(account.pubkey);
            }
        });
        size + 1
            + get_size_of_compressed_u16(ix.accounts.len())
            + ix.accounts.len()
            + get_size_of_compressed_u16(ix.data.len())
            + ix.data.len()
    });

    let mut can_lookups = accounts
        .iter()
        .filter(|pubkey| !programs.contains(pubkey) && !signers.contains(pubkey))
        .copied()
        .collect::<HashSet<_>>();

    let mut lookup_table_addresses = 0;
    let num_of_address_lookups = if let Some(alts) = luts {
        let total_accounts = accounts.len();
        for alt in alts.values() {
            let mut used = false;
            for pubkey in alt {
                if can_lookups.remove(pubkey) {
                    used = true;
                }
            }
            if used {
                lookup_table_addresses += 1;
            }
        }
        accounts = can_lookups
            .union(&signers)
            .chain(programs.iter())
            .copied()
            .collect();
        total_accounts - accounts.len()
    } else {
        0
    };

    let size = get_size_of_compressed_u16(signers.len())
        + signers.len() * 64
        + 3
        + get_size_of_compressed_u16(accounts.len())
        + accounts.len() * 32
        + 32
        + get_size_of_compressed_u16(ixs.len())
        + ixs_size
        + num_of_address_lookups;
    if is_versioned_transaction {
        size + 1 + get_size_of_compressed_u16(0) + lookup_table_addresses * (32 + 2)
    } else {
        size
    }
}
