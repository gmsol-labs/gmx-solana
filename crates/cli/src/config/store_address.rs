use gmsol_sdk::{programs::anchor_lang::prelude::Pubkey, utils::serde::StringPubkey};

#[derive(Debug, clap::Args, serde::Serialize, serde::Deserialize, Default, Clone)]
pub(crate) struct StoreAddress {
    /// The address of the `Store` account.
    #[arg(long, env)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    store: Option<StringPubkey>,
    /// The key fo the `Store` account to use.
    #[arg(long, default_value = "")]
    #[serde(default, skip_serializing_if = "str::is_empty")]
    store_key: String,
}

impl StoreAddress {
    pub(crate) fn address(&self, store_program_id: &Pubkey) -> Pubkey {
        match self.store {
            Some(address) => address.0,
            None => gmsol_sdk::pda::find_store_address(&self.store_key, store_program_id).0,
        }
    }
}
