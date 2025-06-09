use anchor_lang::prelude::*;

/// The Chainlink Program.
pub struct Chainlink;

anchor_lang::solana_program::declare_id!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");

impl Id for Chainlink {
    fn id() -> Pubkey {
        ID
    }
}
