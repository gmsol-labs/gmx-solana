use std::{
    collections::{hash_map, HashMap, HashSet},
    ops::Deref,
};

use solana_sdk::{address_lookup_table::AddressLookupTableAccount, pubkey::Pubkey};

/// Address Lookup Tables.
#[derive(Debug, Clone, Default)]
pub struct AddressLookupTables {
    pub(crate) luts: HashMap<Pubkey, Vec<Pubkey>>,
}

impl AddressLookupTables {
    /// Returns unique addresses.
    pub fn addresses(&self) -> HashSet<Pubkey> {
        self.luts
            .values()
            .flatten()
            .copied()
            .collect::<HashSet<_>>()
    }

    /// Returns whether the LUT list is empty.
    pub fn is_empty(&self) -> bool {
        self.luts.is_empty()
    }

    /// Returns the number of LUTs.
    pub fn len(&self) -> usize {
        self.luts.len()
    }

    /// Add a LUT.
    pub fn add(&mut self, lut: &AddressLookupTableAccount) {
        self.luts.insert(lut.key, lut.addresses.clone());
    }

    /// Returns an iterator of accounts.
    pub fn accounts(&self) -> impl Iterator<Item = AddressLookupTableAccount> + '_ {
        self.luts
            .iter()
            .map(|(key, addresses)| AddressLookupTableAccount {
                key: *key,
                addresses: addresses.clone(),
            })
    }
}

impl FromIterator<(Pubkey, Vec<Pubkey>)> for AddressLookupTables {
    fn from_iter<T: IntoIterator<Item = (Pubkey, Vec<Pubkey>)>>(iter: T) -> Self {
        Self {
            luts: FromIterator::from_iter(iter),
        }
    }
}

impl Extend<(Pubkey, Vec<Pubkey>)> for AddressLookupTables {
    fn extend<T: IntoIterator<Item = (Pubkey, Vec<Pubkey>)>>(&mut self, iter: T) {
        self.luts.extend(iter);
    }
}

impl IntoIterator for AddressLookupTables {
    type IntoIter = hash_map::IntoIter<Pubkey, Vec<Pubkey>>;
    type Item = (Pubkey, Vec<Pubkey>);

    fn into_iter(self) -> Self::IntoIter {
        self.luts.into_iter()
    }
}

impl Deref for AddressLookupTables {
    type Target = HashMap<Pubkey, Vec<Pubkey>>;

    fn deref(&self) -> &Self::Target {
        &self.luts
    }
}
